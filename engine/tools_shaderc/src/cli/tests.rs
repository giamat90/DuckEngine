use super::*;
use crate::null::NullCompiler;
use crate::process::WorkDir;
use assets::dshd::{Backend, Program};

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(ToString::to_string).collect()
}

fn must<T, E>(r: Result<T, E>) -> T {
    r.unwrap_or_else(|_| unreachable!())
}

#[test]
fn stages_come_from_the_extension() {
    assert_eq!(stage_of("a.vert"), Some(Stage::Vertex));
    assert_eq!(stage_of("dir/a.frag"), Some(Stage::Fragment));
    assert_eq!(stage_of("A.VERT"), Some(Stage::Vertex));
    assert_eq!(stage_of("b.Frag"), Some(Stage::Fragment));
    assert_eq!(stage_of("a.glsl"), None);
    assert_eq!(stage_of("vert"), None);
    assert_eq!(stage_of(""), None);
}

#[test]
fn the_backend_defaults_to_glsl_and_can_be_spirv() {
    let default = parse_args(&strings(&["a.vert", "-o", "o.dshd"]));
    assert_eq!(default.map(|a| a.backend), Ok(Backend::Glsl450));
    for (name, want) in [("glsl", Backend::Glsl450), ("spirv", Backend::SpirV)] {
        let args = parse_args(&strings(&["--backend", name, "a.vert", "-o", "o.dshd"]));
        assert_eq!(args.map(|a| a.backend), Ok(want), "{name}");
    }
}

#[test]
fn a_repeated_backend_keeps_the_last_one() {
    let args = parse_args(&strings(&[
        "--backend",
        "spirv",
        "--backend",
        "glsl",
        "a.vert",
        "-o",
        "o",
    ]));
    assert_eq!(args.map(|a| a.backend), Ok(Backend::Glsl450));
}

#[test]
fn an_unknown_or_missing_backend_is_a_usage_error() {
    for bad in [
        strings(&["--backend", "dxil", "a.vert", "-o", "o"]),
        strings(&["--backend"]),
        strings(&["a.vert", "-o", "o", "--backend"]),
    ] {
        assert_eq!(parse_args(&bad).err(), Some(CliError::Usage), "{bad:?}");
    }
}

#[test]
fn build_writes_a_spirv_program_when_asked() {
    let dir = must(WorkDir::create());
    let out = dir.path().join("o.spv.dshd");
    must(fs::write(dir.path().join("a.vert"), "void main() {}\n"));
    must(fs::write(dir.path().join("a.frag"), "void main() {}\n"));
    let mut args = args_for(
        dir.path(),
        &[(Stage::Vertex, "a.vert"), (Stage::Fragment, "a.frag")],
        "o.spv.dshd",
    );
    args.backend = Backend::SpirV;
    assert_eq!(build(&NullCompiler, &args), Ok(()));
    let bytes = std::fs::read(&out).unwrap_or_default();
    let program = Program::parse(&bytes);
    assert!(
        program.is_ok_and(|p| p.backend() == Backend::SpirV && p.module_count() == 2),
        "not a SPIR-V program"
    );
}

#[test]
fn a_full_command_line_is_parsed() {
    let args = parse_args(&strings(&[
        "--tools", "t", "a.vert", "b.frag", "-o", "out.dshd",
    ]));
    assert_eq!(
        args,
        Ok(Args {
            tools: Some(PathBuf::from("t")),
            inputs: vec![
                (Stage::Vertex, "a.vert".to_string()),
                (Stage::Fragment, "b.frag".to_string())
            ],
            output: "out.dshd".to_string(),
            backend: Backend::Glsl450,
        })
    );
    let args = must(parse_args(&strings(&["-o", "o", "a.vert"])));
    assert_eq!(args.tools, None);
}

#[test]
fn wrong_command_lines_are_usage_errors() {
    for bad in [
        &[][..],
        &["a.vert"],
        &["-o", "o"],
        &["a.vert", "-o"],
        &["--tools"],
        &["--tools", "t"],
        &["--nope", "a.vert", "-o", "o"],
        &["-x.vert", "-o", "o"],
    ] {
        assert_eq!(parse_args(&strings(bad)), Err(CliError::Usage), "{bad:?}");
    }
}

#[test]
fn an_input_with_an_unknown_extension_gets_a_clear_message() {
    let result = parse_args(&strings(&["a.txt", "-o", "o"]));
    let text = result
        .as_ref()
        .err()
        .map(ToString::to_string)
        .unwrap_or_default();
    assert!(matches!(result, Err(CliError::BadExtension(_))));
    assert!(text.contains("a.txt") && text.contains(".vert") && text.contains(".frag"));
}

#[test]
fn every_error_has_its_own_message() {
    let all = [
        CliError::Usage,
        CliError::Read("r".to_string(), "why-r".to_string()),
        CliError::BadExtension("x".to_string()),
        CliError::Write("w".to_string(), "why-w".to_string()),
        CliError::Compile("c".to_string(), CompileError::EmptySource),
        CliError::Pack(DshdError::BadModuleCount),
    ];
    let texts: Vec<String> = all.iter().map(ToString::to_string).collect();
    for (i, a) in texts.iter().enumerate() {
        assert!(!a.is_empty());
        assert!(texts[i + 1..].iter().all(|b| a != b));
    }
    assert!(texts[1].contains("why-r") && texts[3].contains("why-w") && texts[4].starts_with("c:"));
}

#[test]
fn atomic_write_replaces_the_file_and_leaves_no_temporary() {
    let dir = must(WorkDir::create());
    let target = dir.path().join("o.bin");
    assert!(write_atomic(&target, b"one").is_ok());
    assert!(write_atomic(&target, b"two").is_ok());
    assert_eq!(fs::read(&target).unwrap_or_default(), b"two");
    assert_eq!(fs::read_dir(dir.path()).map_or(0, Iterator::count), 1);
}

#[test]
fn a_failed_atomic_write_reports_and_leaves_nothing() {
    let dir = must(WorkDir::create());
    let target = dir.path().join("missing-dir").join("o.bin");
    assert!(write_atomic(&target, b"x").is_err());
    assert_eq!(fs::read_dir(dir.path()).map_or(1, Iterator::count), 0);
}

#[test]
fn a_failed_rename_removes_the_temporary_and_keeps_the_cause() {
    let dir = must(WorkDir::create());
    let target = dir.path().join("target-is-a-directory");
    must(fs::create_dir(&target));
    let result = write_atomic(&target, b"x");
    assert!(result.is_err_and(|why| !why.is_empty()));
    assert_eq!(fs::read_dir(dir.path()).map_or(0, Iterator::count), 1);
}

fn args_for(dir: &Path, inputs: &[(Stage, &str)], output: &str) -> Args {
    Args {
        tools: None,
        inputs: inputs
            .iter()
            .map(|(s, n)| (*s, dir.join(n).to_string_lossy().into_owned()))
            .collect(),
        output: dir.join(output).to_string_lossy().into_owned(),
        backend: Backend::Glsl450,
    }
}

#[test]
fn build_writes_a_readable_program() {
    let dir = must(WorkDir::create());
    must(fs::write(dir.path().join("a.vert"), "void main() {}\n"));
    must(fs::write(dir.path().join("b.frag"), "void main() {}\n"));
    let args = args_for(
        dir.path(),
        &[(Stage::Vertex, "a.vert"), (Stage::Fragment, "b.frag")],
        "o.dshd",
    );
    assert_eq!(build(&NullCompiler, &args), Ok(()));
    let bytes = fs::read(&args.output).unwrap_or_default();
    let program = must(Program::parse(&bytes));
    assert_eq!(program.module_count(), 2);
}

#[test]
fn build_writes_nothing_when_an_input_is_missing_or_empty_or_duplicated() {
    let dir = must(WorkDir::create());
    must(fs::write(dir.path().join("a.vert"), "void main() {}\n"));
    must(fs::write(dir.path().join("e.frag"), ""));
    let missing = args_for(dir.path(), &[(Stage::Vertex, "nope.vert")], "o.dshd");
    assert!(matches!(
        build(&NullCompiler, &missing),
        Err(CliError::Read(..))
    ));
    let empty = args_for(dir.path(), &[(Stage::Fragment, "e.frag")], "o.dshd");
    assert!(matches!(
        build(&NullCompiler, &empty),
        Err(CliError::Compile(_, CompileError::EmptySource))
    ));
    let twice = args_for(
        dir.path(),
        &[(Stage::Vertex, "a.vert"), (Stage::Vertex, "a.vert")],
        "o.dshd",
    );
    assert!(matches!(
        build(&NullCompiler, &twice),
        Err(CliError::Pack(_))
    ));
    assert!(!dir.path().join("o.dshd").exists());
}

#[test]
fn run_reports_missing_tools_as_a_tool_problem() {
    let dir = must(WorkDir::create());
    must(fs::write(dir.path().join("a.vert"), "void main() {}\n"));
    let input = dir.path().join("a.vert").to_string_lossy().into_owned();
    let output = dir.path().join("o.dshd").to_string_lossy().into_owned();
    let tools = dir.path().join("no-tools").to_string_lossy().into_owned();
    let result = run(
        &strings(&["--tools", &tools, &input, "-o", &output]),
        &|_| None,
    );
    assert!(matches!(
        result,
        Err(CliError::Compile(_, CompileError::Tool(_)))
    ));
    assert!(!dir.path().join("o.dshd").exists());
}
