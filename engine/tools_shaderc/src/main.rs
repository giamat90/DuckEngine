//! The `shaderc` command: see [`tools_shaderc::cli`].

use std::process::ExitCode;
use tools_shaderc::cli;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match cli::run(&args, &|name| std::env::var_os(name)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
