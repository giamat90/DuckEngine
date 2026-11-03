use std::process::ExitCode;
use tools_assetc::cli::run;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("assetc: {e}");
            ExitCode::FAILURE
        }
    }
}
