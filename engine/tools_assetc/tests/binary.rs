//! Runs the built `tools_assetc` binary: the exit status is the contract of `main`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("assetc-bin-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::create_dir_all(&dir);
    dir
}

fn exit_ok(args: &[&str]) -> bool {
    Command::new(env!("CARGO_BIN_EXE_tools_assetc"))
        .args(args)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

#[test]
fn exits_zero_on_success_and_nonzero_on_failure() {
    let dir = scratch("exit");
    let input = dir.join("in.ppm");
    let output = dir.join("out.dtex");
    let mut bytes = b"P6\n1 1\n255\n".to_vec();
    bytes.extend_from_slice(&[1, 2, 3]);
    assert!(fs::write(&input, bytes).is_ok());
    let (i, o) = (input.to_str().unwrap_or(""), output.to_str().unwrap_or(""));
    assert!(exit_ok(&[i, o]));
    assert!(output.exists());
    assert!(!exit_ok(&[]));
    assert!(!exit_ok(&["missing-file.ppm", o]));
    let _ = fs::remove_dir_all(&dir);
}
