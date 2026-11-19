//! `tools_physics3d_bench icount-workload --layers N --steps N`: the 3D workload of the
//! instruction-count gate (card 8-29). Prints one line; exits with 2 on a bad command line and 1
//! when the world fails.

use std::process::ExitCode;
use tools_physics3d_bench::{parse, run};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let workload = match parse(&args) {
        Ok(workload) => workload,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    match run(workload) {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
