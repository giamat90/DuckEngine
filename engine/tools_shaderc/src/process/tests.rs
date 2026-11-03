use super::*;
use std::ffi::OsString;
use std::time::Duration;

/// The test program itself is the child: `--list` succeeds and prints, an unknown flag fails.
fn me() -> PathBuf {
    must(std::env::current_exe())
}

fn must<T, E>(r: Result<T, E>) -> T {
    r.unwrap_or_else(|_| unreachable!())
}

fn args(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}

#[test]
fn a_successful_child_returns_its_output() {
    let dir = must(WorkDir::create());
    let run = must(run(
        &me(),
        &args(&["--list", "--format", "terse"]),
        dir.path(),
        "ok",
        Duration::from_secs(60),
    ));
    assert!(run.success);
    assert!(
        run.output
            .contains("process::tests::a_successful_child_returns_its_output")
    );
}

#[test]
fn a_failing_child_is_a_value_not_an_error() {
    let dir = must(WorkDir::create());
    let run = must(run(
        &me(),
        &args(&["--definitely-not-a-flag"]),
        dir.path(),
        "bad",
        Duration::from_secs(60),
    ));
    assert!(!run.success);
    assert!(!run.output.is_empty());
}

#[test]
fn a_missing_program_is_a_start_error() {
    let dir = must(WorkDir::create());
    let missing = dir.path().join("no-such-program");
    let result = run(&missing, &[], dir.path(), "missing", Duration::from_secs(5));
    assert!(matches!(result, Err(ToolError::Start(_))));
}

#[test]
#[ignore = "sleeps; run only as the child of the timeout test"]
fn sleeper_for_the_timeout_test() {
    std::thread::sleep(Duration::from_secs(30));
}

#[test]
fn a_child_that_runs_too_long_is_killed() {
    let dir = must(WorkDir::create());
    let started = std::time::Instant::now();
    let result = run(
        &me(),
        &args(&[
            "--ignored",
            "--exact",
            "process::tests::sleeper_for_the_timeout_test",
        ]),
        dir.path(),
        "slow",
        Duration::from_millis(300),
    );
    assert_eq!(result, Err(ToolError::Timeout));
    assert!(started.elapsed() < Duration::from_secs(20));
}

#[test]
fn files_are_read_up_to_a_limit_and_not_beyond() {
    let dir = must(WorkDir::create());
    let path = dir.path().join("big");
    must(std::fs::write(&path, vec![b'a'; 100]));
    assert_eq!(read_bounded(&path, 100).map(|b| b.len()), Ok(100));
    assert_eq!(
        read_bounded(&path, 99).map(|b| b.len()),
        Err(ToolError::TooLarge)
    );
    assert_eq!(read_bounded(&path, 1000).map(|b| b.len()), Ok(100));
    let missing = dir.path().join("nope");
    assert!(matches!(read_bounded(&missing, 10), Err(ToolError::Io(_))));
}

#[test]
fn a_tool_that_prints_too_much_is_an_error_not_an_allocation() {
    let dir = must(WorkDir::create());
    must(std::fs::write(
        dir.path().join("x.out"),
        vec![b'a'; MAX_OUTPUT_LEN + 1],
    ));
    assert_eq!(
        read_output(&dir.path().join("x.out")),
        Err(ToolError::TooLarge)
    );
}

#[test]
fn output_files_use_the_tag() {
    let dir = must(WorkDir::create());
    let _ = run(
        &me(),
        &args(&["--list"]),
        dir.path(),
        "tagged",
        Duration::from_secs(60),
    );
    assert!(dir.path().join("tagged.out").exists());
    assert!(dir.path().join("tagged.err").exists());
}

#[test]
fn the_work_directory_exists_and_is_removed_on_drop() {
    let path;
    {
        let dir = must(WorkDir::create());
        path = dir.path().to_path_buf();
        assert!(path.is_dir());
        assert!(std::fs::write(path.join("f"), b"x").is_ok());
    }
    assert!(!path.exists());
}

#[cfg(unix)]
#[test]
fn the_work_directory_is_private_to_its_owner() {
    use std::os::unix::fs::PermissionsExt;
    let dir = must(WorkDir::create());
    let mode = must(std::fs::metadata(dir.path())).permissions().mode();
    assert_eq!(mode & 0o777, 0o700);
}

#[test]
fn two_work_directories_never_share_a_path() {
    let a = must(WorkDir::create());
    let b = must(WorkDir::create());
    assert_ne!(a.path(), b.path());
}

#[test]
fn every_error_has_its_own_message() {
    let all = [
        ToolError::Start("x".to_string()),
        ToolError::Timeout,
        ToolError::Io("y".to_string()),
        ToolError::TooLarge,
    ];
    let texts: Vec<String> = all.iter().map(ToString::to_string).collect();
    for (i, a) in texts.iter().enumerate() {
        assert!(!a.is_empty());
        assert!(texts[i + 1..].iter().all(|b| a != b));
    }
    assert!(texts[0].contains('x') && texts[2].contains('y'));
}
