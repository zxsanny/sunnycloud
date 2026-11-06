use std::io::{self, BufRead, BufReader, IsTerminal, Write};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// The three standard streams of one command run. Injected, so tests capture each stream apart.
pub struct Terminal {
    stdin: Mutex<Box<dyn BufRead + Send>>,
    stdout: Mutex<Box<dyn Write + Send>>,
    stderr: Mutex<Box<dyn Write + Send>>,
    stdin_is_tty: bool,
}

fn lock<T: ?Sized>(mutex: &Mutex<Box<T>>) -> MutexGuard<'_, Box<T>> {
    // A stream holds no invariant a panicking writer could break halfway.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Terminal {
    pub fn new(
        stdin: Box<dyn BufRead + Send>,
        stdin_is_tty: bool,
        stdout: Box<dyn Write + Send>,
        stderr: Box<dyn Write + Send>,
    ) -> Self {
        Self {
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(stdout),
            stderr: Mutex::new(stderr),
            stdin_is_tty,
        }
    }

    pub fn process() -> Self {
        let stdin = io::stdin();
        let tty = stdin.is_terminal();
        Self::new(
            Box::new(BufReader::new(stdin)),
            tty,
            Box::new(io::stdout()),
            Box::new(io::stderr()),
        )
    }

    pub fn stdin_is_tty(&self) -> bool {
        self.stdin_is_tty
    }

    /// One write, flushed: a line printed here is not split by another writer.
    pub fn write_stdout(&self, text: &str) -> io::Result<()> {
        write_whole(&mut lock(&self.stdout), text)
    }

    pub fn write_stderr(&self, text: &str) -> io::Result<()> {
        write_whole(&mut lock(&self.stderr), text)
    }

    /// The next line of stdin without its line ending; `None` at end of input.
    pub fn read_line(&self) -> io::Result<Option<String>> {
        let mut line = String::new();
        if lock(&self.stdin).read_line(&mut line)? == 0 {
            return Ok(None);
        }
        Ok(Some(line.trim_end_matches(['\r', '\n']).to_owned()))
    }
}

fn write_whole(stream: &mut Box<dyn Write + Send>, text: &str) -> io::Result<()> {
    stream.write_all(text.as_bytes())?;
    stream.flush()
}
