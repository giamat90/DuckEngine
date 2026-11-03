use super::*;
use std::fs;
use std::path::PathBuf;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("assetc-test-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::create_dir_all(&dir);
    dir
}

#[test]
fn parses_plain_and_flagged_arguments() {
    let a = parse_args(&args(&["in.ppm", "out.dtex"])).unwrap_or_else(|_| unreachable!());
    assert_eq!(
        (a.input.as_str(), a.output.as_str()),
        ("in.ppm", "out.dtex")
    );
    assert_eq!(
        a.options,
        Options {
            srgb: false,
            mips: true
        }
    );
    let b =
        parse_args(&args(&["--srgb", "in", "out", "--no-mips"])).unwrap_or_else(|_| unreachable!());
    assert_eq!(
        b.options,
        Options {
            srgb: true,
            mips: false
        }
    );
}

#[test]
fn rejects_wrong_argument_lists() {
    assert_eq!(parse_args(&args(&[])), Err(CliError::Usage));
    assert_eq!(parse_args(&args(&["only"])), Err(CliError::Usage));
    assert_eq!(parse_args(&args(&["a", "b", "c"])), Err(CliError::Usage));
    assert_eq!(
        parse_args(&args(&["a", "b", "--bogus"])),
        Err(CliError::Usage)
    );
    // Two paths and an unknown flag: the flag must be refused, not taken as a path.
    assert_eq!(parse_args(&args(&["--bogus", "b"])), Err(CliError::Usage));
}

#[test]
fn run_converts_a_file() {
    let dir = scratch("ok");
    let input = dir.join("a.ppm");
    let output = dir.join("a.dtex");
    let mut bytes = b"P6\n2 2\n255\n".to_vec();
    bytes.extend_from_slice(&[10; 12]);
    fs::write(&input, bytes).unwrap_or_else(|_| unreachable!());
    let result = run(&args(&[
        input.to_str().unwrap_or(""),
        output.to_str().unwrap_or(""),
    ]));
    assert_eq!(result, Ok(()));
    let written = fs::read(&output).unwrap_or_else(|_| unreachable!());
    assert_eq!(&written[0..4], b"DTEX");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn run_reports_a_missing_input_and_a_bad_image() {
    let dir = scratch("bad");
    let missing = dir.join("missing.ppm");
    let out = dir.join("o.dtex");
    let (m, o) = (missing.to_str().unwrap_or(""), out.to_str().unwrap_or(""));
    assert_eq!(run(&args(&[m, o])), Err(CliError::Read));
    let junk = dir.join("junk.bin");
    fs::write(&junk, b"junk").unwrap_or_else(|_| unreachable!());
    assert_eq!(
        run(&args(&[junk.to_str().unwrap_or(""), o])),
        Err(CliError::Convert(ConvertError::UnknownFormat))
    );
    assert!(!out.exists());
    let unwritable = dir.join("no-such-dir").join("o.dtex");
    let mut bytes = b"P6\n1 1\n255\n".to_vec();
    bytes.extend_from_slice(&[1, 2, 3]);
    let good = dir.join("g.ppm");
    fs::write(&good, bytes).unwrap_or_else(|_| unreachable!());
    assert_eq!(
        run(&args(&[
            good.to_str().unwrap_or(""),
            unwritable.to_str().unwrap_or("")
        ])),
        Err(CliError::Write)
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn every_cli_error_has_a_message() {
    for e in [
        CliError::Usage,
        CliError::Read,
        CliError::Write,
        CliError::Convert(ConvertError::BadHeader),
    ] {
        assert!(!e.to_string().is_empty());
    }
}
