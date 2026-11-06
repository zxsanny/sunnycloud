//! Windows endpoint: the daemon's named pipe, trusted only if the process that serves it
//! runs as the current user. Compile-checked on Linux; it has not run on a Windows host.

use std::io;
use std::os::windows::io::AsRawHandle;
use std::path::PathBuf;
use std::ptr;

use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_PIPE_BUSY, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::SECURITY_IDENTIFICATION;
use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::super::{ClientConfig, ClientError, LocateError};

pub(crate) type Stream = NamedPipeClient;

pub(crate) fn default_endpoint() -> Result<PathBuf, LocateError> {
    let sid = current_user()
        .and_then(|user| user.to_text())
        .map_err(|e| LocateError::UserSid(e.to_string()))?;
    Ok(PathBuf::from(format!(r"\\.\pipe\sunnycloud-daemon-{sid}")))
}

/// Opens the pipe with the identification impersonation level (the server can learn who
/// connected but cannot act as the user), then compares the SID of the server process
/// with the current user's before anything is written. A server whose process cannot be
/// inspected is not trusted.
pub(crate) async fn connect(config: &ClientConfig) -> Result<NamedPipeClient, ClientError> {
    let client = ClientOptions::new()
        .security_qos_flags(SECURITY_IDENTIFICATION)
        .open(&config.endpoint)
        .map_err(open_error)?;
    verify_server(&client)?;
    Ok(client)
}

fn open_error(error: io::Error) -> ClientError {
    match error.kind() {
        io::ErrorKind::NotFound => ClientError::DaemonNotRunning,
        io::ErrorKind::PermissionDenied => ClientError::EndpointUntrusted(
            "the pipe cannot be opened by the current user".to_owned(),
        ),
        _ if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
            ClientError::Io("every instance of the pipe is busy".to_owned())
        }
        _ => ClientError::Io(format!("opening the pipe: {error}")),
    }
}

fn verify_server(client: &NamedPipeClient) -> Result<(), ClientError> {
    let mut server_pid = 0_u32;
    // SAFETY: the handle is the open pipe; `server_pid` is an out pointer.
    if unsafe { GetNamedPipeServerProcessId(client.as_raw_handle(), &mut server_pid) } == 0 {
        return Err(ClientError::EndpointUntrusted(format!(
            "the pipe server process cannot be identified: {}",
            io::Error::last_os_error()
        )));
    }
    let server = process_user(server_pid).map_err(|e| {
        ClientError::EndpointUntrusted(format!("the pipe server user cannot be read: {e}"))
    })?;
    let me = current_user()
        .map_err(|e| ClientError::Io(format!("reading the current user SID: {e}")))?;
    // SAFETY: both SIDs point into live buffers owned by `server` and `me`.
    if unsafe { EqualSid(server.sid(), me.sid()) } == 0 {
        return Err(ClientError::EndpointUntrusted(
            "the pipe server runs as another user".to_owned(),
        ));
    }
    Ok(())
}

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by this program and is closed once.
        if unsafe { CloseHandle(self.0) } == 0 {
            tracing::error!("a process or token handle could not be closed");
        }
    }
}

/// A `TOKEN_USER` and the SID it points to, in one buffer.
struct UserSid {
    /// u64 elements give the 8-byte alignment `TOKEN_USER` requires.
    buffer: Vec<u64>,
}

impl UserSid {
    fn of_token(token: &Handle) -> io::Result<Self> {
        let mut needed = 0_u32;
        // SAFETY: a zero-length query only reports the required size.
        unsafe { GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut needed) };
        let mut buffer = vec![0_u64; (needed as usize).div_ceil(8).max(1)];
        // SAFETY: `buffer` holds at least `needed` bytes.
        let read = unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * 8) as u32,
                &mut needed,
            )
        };
        if read == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { buffer })
    }

    fn sid(&self) -> *mut std::ffi::c_void {
        // SAFETY: after a successful read the buffer starts with a TOKEN_USER whose SID
        // points into the same buffer.
        unsafe { (*self.buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }

    fn to_text(&self) -> io::Result<String> {
        let mut text: *mut u16 = ptr::null_mut();
        // SAFETY: the SID is valid; `text` receives a LocalAlloc'd NUL-terminated string.
        if unsafe { ConvertSidToStringSidW(self.sid(), &mut text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `text` is NUL-terminated, so scanning to the terminator stays in bounds.
        let sid = unsafe {
            let mut len = 0;
            while *text.add(len) != 0 {
                len += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(text, len))
        };
        // SAFETY: `text` came from ConvertSidToStringSidW and is freed once.
        if !unsafe { LocalFree(text.cast()) }.is_null() {
            tracing::error!("the SID string could not be freed");
        }
        Ok(sid)
    }
}

fn token_user(process: HANDLE) -> io::Result<UserSid> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: `process` is a valid process handle; `token` is an out pointer.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    UserSid::of_token(&Handle(token))
}

fn current_user() -> io::Result<UserSid> {
    // SAFETY: the current-process pseudo handle is always valid and needs no closing.
    token_user(unsafe { GetCurrentProcess() })
}

fn process_user(pid: u32) -> io::Result<UserSid> {
    // SAFETY: plain call; the returned handle is owned by `Handle`.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    let process = Handle(process);
    token_user(process.0)
}
