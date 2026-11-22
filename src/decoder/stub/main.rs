//! `decoder-test-stub`: the one binary that stands in for `ffmpeg`, `ffprobe`, `vips`, `vipsheader`
//! and `vipsthumbnail` in the `test-stubs` image variant (10 description 13.4). Built only with
//! the `test-freeze` feature, so no release build contains it.
//!
//! A marker in the input (see [`marker`]) selects hostile behaviour. Without a marker the stub
//! appends its argv, environment and limits as one JSON line to `<scratch>/_stub/<job id>.jsonl`
//! and runs the real tool, which the image keeps as `<name>.real` beside it. The scratch root and
//! the job id come from the working directory the runner gives every child: `<scratch>/<job id>/<dir>`.

// The writer half of the module (`text`, `jpeg`, `mp4`) belongs to the container suite, which includes the same file.
#[allow(dead_code)]
mod marker;

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::thread;
use std::time::Duration;

use marker::{Behaviour, SCAN_LEN};

/// The facts `vipsheader` gives for a marked input: small enough for any pixel limit.
const MARKED_SIDE: &str = "64";
const FAILED: u8 = 70;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    match run(&args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("decoder-test-stub: {error}");
            ExitCode::from(FAILED)
        }
    }
}

fn run(args: &[OsString]) -> io::Result<ExitCode> {
    let program = PathBuf::from(&args[0]);
    let tool = program
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_owned();
    let rest = &args[1..];
    let marked = marker_of(rest)?;
    match (tool.as_str(), marked) {
        ("vipsheader", Some(_)) => {
            println!("{MARKED_SIDE}");
            Ok(ExitCode::SUCCESS)
        }
        ("vips" | "vipsthumbnail" | "ffmpeg", Some(behaviour)) => act(&behaviour),
        _ => {
            log_call(&tool, args)?;
            Err(run_real(&program, rest))
        }
    }
}

/// The marker in the input the child was given, if any.
fn marker_of(args: &[OsString]) -> io::Result<Option<Behaviour>> {
    let Some(input) = args
        .iter()
        .find(|arg| arg.to_string_lossy().starts_with("/proc/self/fd/"))
    else {
        return Ok(None);
    };
    let mut head = Vec::new();
    File::open(input)?
        .take(SCAN_LEN as u64)
        .read_to_end(&mut head)?;
    Ok(marker::parse(&head))
}

/// `<dir>/<name>.real` is the tool the image kept; `exec` returns only on failure.
fn run_real(program: &Path, args: &[OsString]) -> io::Error {
    let mut real = program.as_os_str().to_owned();
    real.push(".real");
    Command::new(&real).arg0(&real).args(args).exec()
}

/// `(scratch root, job id)` from the working directory `<scratch>/<job id>/<dir>`; `None` when the
/// stub runs outside a job (a tool started by hand has no job directory above it).
fn job_location() -> Option<(PathBuf, String)> {
    let cwd = std::env::current_dir().ok()?;
    let job = cwd.parent()?;
    let scratch = job.parent()?;
    let id = job.file_name()?.to_str()?;
    Some((scratch.to_path_buf(), id.to_owned()))
}

fn required_location() -> io::Result<(PathBuf, String)> {
    job_location().ok_or_else(|| io::Error::other("the stub must run in <scratch>/<job id>/<dir>"))
}

/// Without a marker, one JSON line per call in `<scratch>/_stub/<job id>.jsonl` when that directory
/// exists (the suite makes it); a call outside a job is not logged.
fn log_call(tool: &str, args: &[OsString]) -> io::Result<()> {
    let Some((scratch, job)) = job_location() else {
        return Ok(());
    };
    let dir = scratch.join("_stub");
    if !dir.is_dir() {
        return Ok(());
    }
    let line = serde_json::json!({
        "tool": tool,
        "argv": args.iter().map(|arg| arg.to_string_lossy()).collect::<Vec<_>>(),
        "env": std::env::vars().collect::<Vec<_>>(),
        "limits": fs::read_to_string("/proc/self/limits")?,
    });
    let mut log = OpenOptions::new()
        .append(true)
        .create(true)
        .open(dir.join(format!("{job}.jsonl")))?;
    writeln!(log, "{line}")
}

fn act(behaviour: &Behaviour) -> io::Result<ExitCode> {
    match behaviour {
        Behaviour::Hang => sleep_forever(),
        Behaviour::Alloc(mib) => {
            let mut memory = vec![0_u8; usize::try_from(*mib << 20).map_err(io::Error::other)?];
            for at in (0..memory.len()).step_by(4096) {
                memory[at] = 1;
            }
            // Held until the guard kills the process.
            std::hint::black_box(&memory);
            sleep_forever()
        }
        Behaviour::IgnoreTerm => {
            set_handler(libc::SIGTERM, libc::SIG_IGN)?;
            sleep_forever()
        }
        Behaviour::SetsidFork(next) => escape_and_leak(next),
        Behaviour::WriteNextJob(next) => write_next_job(next),
        Behaviour::Ptrace => probe_runner(),
        Behaviour::Stderr(kib) => {
            let line = b"FAKE-SECRET-0123456789 \x1b[31mred\x1b[0m line with \r control\n";
            let mut stderr = io::stderr().lock();
            let mut written = 0;
            while written < kib * 1024 {
                stderr.write_all(line)?;
                written += line.len() as u64;
            }
            stderr.flush()?;
            sleep_forever()
        }
        Behaviour::Write(mib) => {
            let mut out = File::create("result.webp")?;
            let block = vec![0_u8; 1 << 20];
            for _ in 0..*mib {
                out.write_all(&block)?;
            }
            Ok(ExitCode::SUCCESS)
        }
        Behaviour::Spin => {
            let cores = thread::available_parallelism().map_or(2, usize::from);
            for _ in 1..cores {
                thread::spawn(spin);
            }
            spin();
            sleep_forever()
        }
        Behaviour::Segv => {
            set_handler(libc::SIGSEGV, libc::SIG_DFL)?;
            // SAFETY: raising a signal on the current process takes two integers and no pointers.
            unsafe { libc::raise(libc::SIGSEGV) };
            sleep_forever()
        }
        Behaviour::Exit(code) => Ok(ExitCode::from(u8::try_from(*code).unwrap_or(FAILED))),
    }
}

fn sleep_forever() -> ! {
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

fn spin() {
    let mut counter = 0_u64;
    loop {
        counter = std::hint::black_box(counter.wrapping_add(1));
    }
}

fn set_handler(signal: libc::c_int, handler: libc::sighandler_t) -> io::Result<()> {
    // SAFETY: `signal` takes a signal number and SIG_IGN or SIG_DFL; no pointers are involved.
    if unsafe { libc::signal(signal, handler) } == libc::SIG_ERR {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// IT-10-14: leave the process group and the session, double fork, and in the grandchild keep
/// trying to read the next job's input. The first process waits until the grandchild is set up
/// and then ends, so the runner finds the escaped process as a descendant, never as a member of
/// the tool's own group.
fn escape_and_leak(next: &str) -> io::Result<ExitCode> {
    let (scratch, _) = required_location()?;
    let input = scratch.join(next).join("in").join("source");
    let leak = scratch.join("leak.txt");
    let mut ready = [0; 2];
    // SAFETY: `pipe` fills two descriptors of a valid array.
    if unsafe { libc::pipe(ready.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the stub is single threaded here; the child only calls async-signal-safe functions
    // until it returns into code that is allowed after a fork of a single thread.
    match unsafe { libc::fork() } {
        -1 => Err(io::Error::last_os_error()),
        0 => {
            // SAFETY: `setsid` and `fork` take no pointers.
            unsafe { libc::setsid() };
            // SAFETY: as above.
            if unsafe { libc::fork() } == 0 {
                set_handler(libc::SIGTERM, libc::SIG_IGN)?;
                set_handler(libc::SIGHUP, libc::SIG_IGN)?;
                // SAFETY: writes one byte from a valid buffer to the pipe this process owns.
                unsafe { libc::write(ready[1], b"r".as_ptr().cast(), 1) };
                loop {
                    if File::open(&input).is_ok() {
                        let mut file = OpenOptions::new().append(true).create(true).open(&leak)?;
                        writeln!(file, "LEAK")?;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        _ => {
            let mut byte = [0_u8; 1];
            // SAFETY: reads at most one byte into a valid buffer from the pipe this process owns.
            unsafe { libc::read(ready[0], byte.as_mut_ptr().cast(), 1) };
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// IT-10-14, variant: a running child tries to modify the next job's input; the answer goes
/// to `out/write-next.txt`.
fn write_next_job(next: &str) -> io::Result<ExitCode> {
    let (scratch, _) = required_location()?;
    let input = scratch.join(next).join("in").join("source");
    let answer = match OpenOptions::new().write(true).open(&input) {
        Ok(_) => "opened for writing".to_owned(),
        Err(error) => format!("errno {}", error.raw_os_error().unwrap_or(0)),
    };
    fs::write("write-next.txt", answer)?;
    Ok(ExitCode::from(1))
}

/// IT-10-15: what a child running as the runner's own user can do to the runner. One line per
/// attempt in `out/ptrace.txt`: `<name> ok` or `<name> errno <n>`.
fn probe_runner() -> io::Result<ExitCode> {
    // SAFETY: `getppid` takes no arguments.
    let runner = unsafe { libc::getppid() };
    let mut report = String::new();
    // SAFETY: PTRACE_ATTACH takes a pid and two ignored arguments; the call may only fail or attach.
    let attach = unsafe { libc::ptrace(libc::PTRACE_ATTACH, runner, 0_usize, 0_usize) };
    report.push_str(&outcome("ptrace_attach", attach >= 0));
    let mut byte = [0_u8; 1];
    let local = libc::iovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: 1,
    };
    let remote = libc::iovec {
        iov_base: std::ptr::dangling_mut(),
        iov_len: 1,
    };
    // SAFETY: both iovecs describe valid or inert memory; the remote address is never ours to read.
    let read = unsafe { libc::process_vm_readv(runner, &local, 1, &remote, 1, 0) };
    report.push_str(&outcome("process_vm_readv", read >= 0));
    for file in ["mem", "environ"] {
        let opened = File::open(format!("/proc/{runner}/{file}"));
        report.push_str(&outcome(&format!("open_proc_{file}"), opened.is_ok()));
    }
    // SAFETY: PTRACE_TRACEME ignores its other arguments.
    let traceme = unsafe { libc::ptrace(libc::PTRACE_TRACEME, 0_usize, 0_usize, 0_usize) };
    report.push_str(&outcome("ptrace_traceme", traceme >= 0));
    let status = fs::read_to_string(format!("/proc/{runner}/status"))?;
    for field in ["Seccomp", "NoNewPrivs"] {
        let value = status
            .lines()
            .find_map(|line| line.strip_prefix(field)?.strip_prefix(':'))
            .map_or("missing", str::trim);
        report.push_str(&format!("runner_{field} {value}\n"));
    }
    fs::write("ptrace.txt", report)?;
    Ok(ExitCode::from(1))
}

/// Reads `errno` of the call just made; nothing may run between the call and this.
fn outcome(name: &str, succeeded: bool) -> String {
    if succeeded {
        format!("{name} ok\n")
    } else {
        let errno = io::Error::last_os_error().raw_os_error().unwrap_or(0);
        format!("{name} errno {errno}\n")
    }
}
