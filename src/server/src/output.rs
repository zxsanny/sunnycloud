//! The writers: text to stdout or one JSON object, diagnostics to stderr, the one-time secret once on stdout.

use std::io;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::error::CliError;
use crate::exit::Exit;
use crate::outcome::Outcome;
use crate::terminal::Terminal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Text,
    Json,
}

#[derive(Serialize)]
struct Envelope<'a> {
    result: &'a str,
    code: &'a str,
    data: Map<String, Value>,
}

pub fn print_outcome(terminal: &Terminal, mode: Mode, outcome: Outcome) -> io::Result<()> {
    let Outcome {
        class,
        mut data,
        secret,
        document,
    } = outcome;
    if let Some(document) = document {
        return terminal.write_stdout(&document);
    }
    let secret = secret.map(|secret| secret.reveal());
    let text = match mode {
        Mode::Text => {
            let mut text = facts(&data);
            if let Some((label, value)) = &secret {
                push_fact(&mut text, label, &Value::String(value.clone()));
            }
            text
        }
        Mode::Json => {
            if let Some((label, value)) = secret {
                data.insert(label.to_owned(), Value::String(value));
            }
            envelope(class.name(), class.name(), data)?
        }
    };
    terminal.write_stdout(&text)
}

/// Text mode: the message on stderr, nothing on stdout. `--json`: the envelope on stdout as well, except for
/// exit 3, which is decided before any result exists.
pub fn print_error(terminal: &Terminal, mode: Mode, error: &CliError) -> io::Result<()> {
    terminal.write_stderr(&format!("error: {error}\n"))?;
    if mode == Mode::Json && error.exit() != Exit::Usage {
        let mut data = Map::new();
        data.insert("message".to_owned(), Value::String(error.to_string()));
        terminal.write_stdout(&envelope("failed", error.code(), data)?)?;
    }
    Ok(())
}

fn envelope(result: &str, code: &str, data: Map<String, Value>) -> io::Result<String> {
    let mut text =
        serde_json::to_string(&Envelope { result, code, data }).map_err(io::Error::other)?;
    text.push('\n');
    Ok(text)
}

fn facts(data: &Map<String, Value>) -> String {
    let mut text = String::new();
    for (key, value) in data {
        push_fact(&mut text, key, value);
    }
    text
}

fn push_fact(text: &mut String, key: &str, value: &Value) {
    text.push_str(key);
    text.push_str(": ");
    match value {
        Value::String(value) => text.push_str(value),
        other => text.push_str(&other.to_string()),
    }
    text.push('\n');
}
