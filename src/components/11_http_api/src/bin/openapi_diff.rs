//! `openapi_diff print` writes the document of this build to stdout; `openapi_diff diff OLD NEW` compares two
//! document files and exits 1 when `NEW` removes or tightens anything `OLD` offered (exit 2 on a read error).

use std::process::ExitCode;

use serde_json::Value;
use sunnycloud_http_api::{current_document, diff_documents};

fn load(path: &str) -> Result<Value, String> {
    let bytes = std::fs::read(path).map_err(|failure| format!("cannot read {path}: {failure}"))?;
    serde_json::from_slice(&bytes).map_err(|failure| format!("{path} is not JSON: {failure}"))
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    match args {
        [command] if command == "print" => {
            let document = current_document();
            println!("{}", String::from_utf8_lossy(document.as_bytes()));
            Ok(ExitCode::SUCCESS)
        }
        [command, old, new] if command == "diff" => {
            let breaking = diff_documents(&load(old)?, &load(new)?);
            for change in &breaking {
                println!("BREAKING {change}");
            }
            Ok(if breaking.is_empty() {
                println!("additive-only: ok");
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        _ => Err("usage: openapi_diff print | openapi_diff diff OLD.json NEW.json".to_owned()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}
