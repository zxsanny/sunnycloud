//! Windows endpoint: a named pipe whose DACL grants access to the current user SID only,
//! that refuses remote clients, and whose first instance must be created by this process.
//! Compile-checked on Linux; it has not run on a Windows host (task AC-2, H-WIN).

use std::ffi::c_void;
use std::io;
use std::ptr;
use std::sync::Arc;

use ipc_schema::DaemonControl;
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::error::EndpointRefused;
use super::server::{IpcServer, Listener};

/// Serves `daemon` on `\\.\pipe\sunnycloud-daemon-<current user SID>`.
pub async fn start<D>(daemon: Arc<D>) -> Result<IpcServer, EndpointRefused>
where
    D: DaemonControl + Send + Sync + 'static,
{
    match bind() {
        Ok(endpoint) => {
            let name = endpoint.name.clone();
            Ok(IpcServer::spawn(daemon, endpoint, name))
        }
        Err(refused) => {
            tracing::error!(reason = %refused, "endpoint refused");
            Err(refused)
        }
    }
}

fn bind() -> Result<PipeEndpoint, EndpointRefused> {
    let sid = current_user_sid().map_err(|e| EndpointRefused::io("reading", "the user SID", e))?;
    let name = format!(r"\\.\pipe\sunnycloud-daemon-{sid}");
    let security = PipeSecurity::for_sid(&sid)
        .map_err(|e| EndpointRefused::io("building the security descriptor for", &name, e))?;
    let first = security.create(&name, true).map_err(|e| {
        // FILE_FLAG_FIRST_PIPE_INSTANCE: Windows answers ERROR_ACCESS_DENIED when the
        // name is already taken, which is how a squatted pipe shows up.
        if e.kind() == io::ErrorKind::PermissionDenied {
            EndpointRefused::PipeExists { name: name.clone() }
        } else {
            EndpointRefused::io("creating", &name, e)
        }
    })?;
    Ok(PipeEndpoint {
        name,
        security,
        next: Some(first),
    })
}

struct PipeEndpoint {
    name: String,
    security: PipeSecurity,
    /// The instance waiting for the next client.
    next: Option<NamedPipeServer>,
}

impl Listener for PipeEndpoint {
    type Stream = NamedPipeServer;

    async fn accept(&mut self) -> io::Result<NamedPipeServer> {
        let waiting = match self.next.take() {
            Some(waiting) => waiting,
            None => self.security.create(&self.name, false)?,
        };
        waiting.connect().await?;
        self.next = Some(self.security.create(&self.name, false)?);
        Ok(waiting)
    }

    fn close(self) -> io::Result<()> {
        Ok(()) // a pipe disappears with its last handle
    }
}

/// The security descriptor every instance of the pipe is created with.
struct PipeSecurity {
    attributes: SECURITY_ATTRIBUTES,
}

// SAFETY: the descriptor behind the pointer is allocated once, never written afterwards
// and freed only in `drop`; sharing the pointer across threads is sound.
unsafe impl Send for PipeSecurity {}

impl PipeSecurity {
    /// `D:P(A;;GA;;;<sid>)`: a protected DACL with one entry, all access for the user.
    fn for_sid(sid: &str) -> io::Result<Self> {
        let sddl: Vec<u16> = format!("D:P(A;;GA;;;{sid})")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut descriptor: *mut c_void = ptr::null_mut();
        // SAFETY: `sddl` is NUL-terminated and outlives the call; `descriptor` receives a
        // LocalAlloc'd block that `drop` frees.
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        if converted == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            attributes: SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            },
        })
    }

    fn create(&self, name: &str, first: bool) -> io::Result<NamedPipeServer> {
        let attributes: *const SECURITY_ATTRIBUTES = &self.attributes;
        // SAFETY: `attributes` points to a valid SECURITY_ATTRIBUTES whose descriptor lives
        // as long as `self`; CreateNamedPipe copies it.
        unsafe {
            ServerOptions::new()
                .first_pipe_instance(first)
                .reject_remote_clients(true)
                .create_with_security_attributes_raw(name, attributes.cast_mut().cast())
        }
    }
}

impl Drop for PipeSecurity {
    fn drop(&mut self) {
        // SAFETY: the descriptor came from ConvertStringSecurityDescriptorToSecurityDescriptorW
        // and is freed exactly once.
        let leaked = unsafe { LocalFree(self.attributes.lpSecurityDescriptor) };
        if !leaked.is_null() {
            tracing::error!("the pipe security descriptor could not be freed");
        }
    }
}

fn current_user_sid() -> io::Result<String> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: the current-process pseudo handle is always valid; `token` is an out pointer.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let sid = sid_of_token(token);
    // SAFETY: `token` was opened above and is closed once.
    if unsafe { CloseHandle(token) } == 0 {
        tracing::error!("the process token handle could not be closed");
    }
    sid
}

fn sid_of_token(token: HANDLE) -> io::Result<String> {
    let mut needed = 0_u32;
    // SAFETY: a zero-length query only reports the required size.
    unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut needed) };
    // u64 elements give the 8-byte alignment TOKEN_USER requires.
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(8).max(1)];
    // SAFETY: `buffer` holds at least `needed` bytes.
    let read = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * 8) as u32,
            &mut needed,
        )
    };
    if read == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: on success the buffer starts with a TOKEN_USER whose SID points into it.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text: *mut u16 = ptr::null_mut();
    // SAFETY: `sid` is valid; `text` receives a LocalAlloc'd NUL-terminated string.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `text` is NUL-terminated, so scanning to the terminator stays in bounds.
    let sid_text = unsafe {
        let mut len = 0;
        while *text.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(text, len))
    };
    // SAFETY: `text` was allocated by ConvertSidToStringSidW and is freed once.
    if !unsafe { LocalFree(text.cast()) }.is_null() {
        tracing::error!("the SID string could not be freed");
    }
    Ok(sid_text)
}
