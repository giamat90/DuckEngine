use super::*;
use std::ffi::OsString;

#[test]
fn pins_are_the_versions_of_the_spike() {
    assert_eq!(GLSLANG_VERSION, "16.6.0");
    assert_eq!(SPIRV_CROSS_REVISION, "vulkan-sdk-1.4.363.0");
}

#[test]
fn glslang_version_is_read_from_the_first_line() {
    let out =
        "Glslang Version: 11:16.6.0\nESSL Version: OpenGL ES GLSL 3.20 glslang Khronos. 16.6.0\n";
    assert_eq!(glslang_version(out), Some("16.6.0"));
    assert_eq!(glslang_version("Glslang Version: 16.6.0"), Some("16.6.0"));
    assert_eq!(
        glslang_version("Glslang Version: 11:16.6.0\r\n"),
        Some("16.6.0")
    );
    assert_eq!(glslang_version("ESSL Version: 3.20\n"), None);
    assert_eq!(glslang_version(""), None);
    assert_eq!(glslang_version("Glslang Version: \n"), None);
}

#[test]
fn spirv_cross_revision_is_read_from_the_commit_line() {
    let out = "Git commit: vulkan-sdk-1.4.363.0 Timestamp: 2026-11-02T17:47:57\n";
    assert_eq!(spirv_cross_revision(out), Some("vulkan-sdk-1.4.363.0"));
    assert_eq!(
        spirv_cross_revision("Git commit: abc123\r\n"),
        Some("abc123")
    );
    assert_eq!(spirv_cross_revision("Git commit: \n"), None);
    assert_eq!(spirv_cross_revision("something else"), None);
}

#[test]
fn version_checks_compare_with_the_pins() {
    assert!(check_glslang("Glslang Version: 11:16.6.0\n").is_ok());
    let wrong = check_glslang("Glslang Version: 11:15.0.0\n");
    assert!(wrong.is_err_and(|e| e.contains("15.0.0") && e.contains("16.6.0")));
    assert!(check_glslang("garbage").is_err());
    assert!(check_spirv_cross("Git commit: vulkan-sdk-1.4.363.0 Timestamp: x\n").is_ok());
    let wrong = check_spirv_cross("Git commit: vulkan-sdk-1.3.0.0 Timestamp: x\n");
    assert!(
        wrong.is_err_and(|e| e.contains("vulkan-sdk-1.3.0.0") && e.contains(SPIRV_CROSS_REVISION))
    );
    assert!(check_spirv_cross("garbage").is_err());
}

#[test]
fn paths_follow_the_layout_under_a_root() {
    let paths = ToolPaths::from_root(Path::new("root"));
    let glslang = paths.glslang.to_string_lossy().replace('\\', "/");
    let cross = paths.spirv_cross.to_string_lossy().replace('\\', "/");
    assert!(
        glslang.contains("/root/tools/glslang/bin/glslang"),
        "{glslang}"
    );
    assert!(cross.contains("/root/build-spirv-cross/"), "{cross}");
    assert!(cross.contains("spirv-cross"), "{cross}");
    let exe = |p: &Path| p.extension().is_some_and(|e| e == "exe");
    assert_eq!(exe(&paths.glslang), cfg!(windows));
    assert_eq!(exe(&paths.spirv_cross), cfg!(windows));
}

#[test]
fn every_tool_path_is_absolute_because_children_run_in_another_directory() {
    assert!(
        ToolPaths::from_root(Path::new("rel/root"))
            .glslang
            .is_absolute()
    );
    assert!(
        ToolPaths::from_root(Path::new("rel/root"))
            .spirv_cross
            .is_absolute()
    );
    let paths = ToolPaths::resolve(&env_of(&[]));
    assert!(paths.glslang.is_absolute() && paths.spirv_cross.is_absolute());
    let paths = ToolPaths::resolve(&env_of(&[
        (ENV_GLSLANG, "my-glslang"),
        (ENV_SPIRV_CROSS, "sub/my-cross"),
    ]));
    assert!(paths.glslang.is_absolute() && paths.glslang.ends_with("my-glslang"));
    assert!(paths.spirv_cross.is_absolute() && paths.spirv_cross.ends_with("sub/my-cross"));
    let cwd = std::env::current_dir().unwrap_or_default();
    assert!(paths.glslang.starts_with(cwd));
}

fn env_of<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
    move |name| {
        pairs
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| OsString::from(v))
    }
}

#[test]
fn resolve_uses_the_default_root_without_environment() {
    let paths = ToolPaths::resolve(&env_of(&[]));
    assert_eq!(paths, ToolPaths::from_root(Path::new(DEFAULT_ROOT)));
}

#[test]
fn resolve_honours_the_root_variable() {
    let paths = ToolPaths::resolve(&env_of(&[(ENV_ROOT, "elsewhere")]));
    assert_eq!(paths, ToolPaths::from_root(Path::new("elsewhere")));
}

#[test]
fn resolve_lets_each_tool_variable_win() {
    let paths = ToolPaths::resolve(&env_of(&[
        (ENV_ROOT, "elsewhere"),
        (ENV_GLSLANG, "my-glslang"),
    ]));
    let from_root = ToolPaths::from_root(Path::new("elsewhere"));
    assert_eq!(paths.glslang, absolutize(Path::new("my-glslang")));
    assert_eq!(paths.spirv_cross, from_root.spirv_cross);
    let paths = ToolPaths::resolve(&env_of(&[(ENV_SPIRV_CROSS, "my-cross")]));
    assert_eq!(paths.spirv_cross, absolutize(Path::new("my-cross")));
    assert_eq!(
        paths.glslang,
        ToolPaths::from_root(Path::new(DEFAULT_ROOT)).glslang
    );
}

fn strings(args: &[OsString]) -> Vec<String> {
    args.iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn argument_vectors_have_one_entry_per_argument() {
    let glslang = glslang_args(Path::new("a b/in.vert"), Path::new("out dir/o.spv"));
    assert_eq!(
        strings(&glslang),
        vec!["-V", "a b/in.vert", "-o", "out dir/o.spv"]
    );
    let glsl = cross_glsl_args(Path::new("x.spv"), Path::new("y.glsl"));
    assert_eq!(
        strings(&glsl),
        vec!["x.spv", "--version", "450", "--no-es", "--output", "y.glsl"]
    );
    let reflect = cross_reflect_args(Path::new("x.spv"), Path::new("y.json"));
    assert_eq!(
        strings(&reflect),
        vec!["x.spv", "--reflect", "--output", "y.json"]
    );
}

#[test]
fn hostile_file_names_stay_single_arguments() {
    let name = "in; rm -rf x && $(evil) \"q\".vert";
    let args = glslang_args(Path::new(name), Path::new("o.spv"));
    assert_eq!(args.len(), 4);
    assert_eq!(args[1], OsString::from(name));
}

#[test]
fn newlines_are_normalised_to_line_feeds() {
    assert_eq!(normalize_newlines("a\r\nb\r\n"), "a\nb\n");
    assert_eq!(normalize_newlines("a\nb\n"), "a\nb\n");
    assert_eq!(normalize_newlines("a\r\n\r\nb"), "a\n\nb");
    assert_eq!(normalize_newlines("lone\rcr"), "lone\rcr");
    assert_eq!(normalize_newlines(""), "");
}

#[test]
fn the_require_variable_means_what_the_gl_tests_mean() {
    assert!(is_required(Some("1")));
    assert!(is_required(Some("yes")));
    assert!(!is_required(Some("")));
    assert!(!is_required(Some("0")));
    assert!(!is_required(None));
}
