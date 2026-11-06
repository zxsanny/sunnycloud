use std::process::ExitCode;

fn main() -> ExitCode {
    sunnycloud_worker::execute(
        std::env::args_os().skip(1).collect(),
        std::env::vars().collect(),
    )
}
