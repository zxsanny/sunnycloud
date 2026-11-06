use std::process::ExitCode;

fn main() -> ExitCode {
    let env = sunnycloud_server::process_env();
    ExitCode::from(sunnycloud_server::execute(
        std::env::args_os().collect(),
        &env,
    ))
}
