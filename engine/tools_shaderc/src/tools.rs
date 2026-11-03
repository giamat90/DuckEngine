//! The external tools: where they are, which versions are accepted, and the exact argument
//! vectors they are run with (ADR-0009: glslang for GLSL to SPIR-V, SPIRV-Cross for SPIR-V to
//! OpenGL GLSL and for reflection).
//!
//! Tools are untrusted downloads kept in a git-ignored folder (`target/shader-spike` by default,
//! see `wiki/spikes/shader-toolchain-spike.md`). Their paths come from the command line or the
//! environment; nothing absolute is compiled in. Arguments are always separate vector entries,
//! never one string handed to a shell.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The glslang version this tool accepts (`THIRD_PARTY.md`, ADR-0009).
pub const GLSLANG_VERSION: &str = "16.6.0";
/// The SPIRV-Cross revision (the Vulkan SDK tag it was built from) this tool accepts.
pub const SPIRV_CROSS_REVISION: &str = "vulkan-sdk-1.4.363.0";

/// Root folder of the tools when nothing says otherwise, relative to the working directory.
pub const DEFAULT_ROOT: &str = "target/shader-spike";
/// Environment variable that sets the root folder.
pub const ENV_ROOT: &str = "DUCK_SHADER_TOOLS";
/// Environment variable that sets the glslang executable.
pub const ENV_GLSLANG: &str = "DUCK_GLSLANG";
/// Environment variable that sets the SPIRV-Cross executable.
pub const ENV_SPIRV_CROSS: &str = "DUCK_SPIRV_CROSS";
/// Environment variable that makes tests fail instead of skip when the tools are missing.
pub const ENV_REQUIRE: &str = "DUCK_REQUIRE_SHADER_TOOLS";

/// The two executables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolPaths {
    /// The glslang command-line compiler.
    pub glslang: PathBuf,
    /// The SPIRV-Cross command-line tool.
    pub spirv_cross: PathBuf,
}

impl ToolPaths {
    /// The layout the spike used under `root`: `tools/glslang/bin/glslang` and
    /// `build-spirv-cross/[Release/]spirv-cross`.
    #[must_use]
    pub fn from_root(root: &Path) -> Self {
        let root = absolutize(root);
        if cfg!(windows) {
            Self {
                glslang: root.join("tools/glslang/bin/glslang.exe"),
                spirv_cross: root.join("build-spirv-cross/Release/spirv-cross.exe"),
            }
        } else {
            Self {
                glslang: root.join("tools/glslang/bin/glslang"),
                spirv_cross: root.join("build-spirv-cross/spirv-cross"),
            }
        }
    }

    /// Paths from the environment: a tool variable wins over the root variable, which wins over
    /// [`DEFAULT_ROOT`]. `env` reads one variable.
    #[must_use]
    pub fn resolve(env: &dyn Fn(&str) -> Option<OsString>) -> Self {
        let root = env(ENV_ROOT).map_or_else(|| PathBuf::from(DEFAULT_ROOT), PathBuf::from);
        let mut paths = Self::from_root(&root);
        if let Some(path) = env(ENV_GLSLANG) {
            paths.glslang = absolutize(Path::new(&path));
        }
        if let Some(path) = env(ENV_SPIRV_CROSS) {
            paths.spirv_cross = absolutize(Path::new(&path));
        }
        paths
    }
}

/// Makes `path` absolute against the current directory (no file system access, no symlink
/// resolution). Tools run with another working directory, so a relative path would break; if the
/// current directory is unknown the path is returned as given.
#[must_use]
pub fn absolutize(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The version in the output of `glslang --version` (`Glslang Version: 11:16.6.0`).
#[must_use]
pub fn glslang_version(output: &str) -> Option<&str> {
    let line = output
        .lines()
        .find_map(|l| l.strip_prefix("Glslang Version:"))?;
    let version = line.rsplit(':').next()?.trim();
    (!version.is_empty()).then_some(version)
}

/// The revision in the output of `spirv-cross --revision` (`Git commit: <tag> Timestamp: ...`).
#[must_use]
pub fn spirv_cross_revision(output: &str) -> Option<&str> {
    let line = output.lines().find_map(|l| l.strip_prefix("Git commit:"))?;
    line.split_whitespace().next()
}

/// Checks the `--version` output of glslang against [`GLSLANG_VERSION`].
///
/// # Errors
/// A message naming the version found and the version expected.
pub fn check_glslang(output: &str) -> Result<(), String> {
    match glslang_version(output) {
        Some(GLSLANG_VERSION) => Ok(()),
        Some(other) => Err(format!(
            "glslang is version {other}, this tool needs {GLSLANG_VERSION}"
        )),
        None => Err("the output of glslang --version is not understood".to_string()),
    }
}

/// Checks the `--revision` output of SPIRV-Cross against [`SPIRV_CROSS_REVISION`].
///
/// # Errors
/// A message naming the revision found and the revision expected.
pub fn check_spirv_cross(output: &str) -> Result<(), String> {
    match spirv_cross_revision(output) {
        Some(SPIRV_CROSS_REVISION) => Ok(()),
        Some(other) => Err(format!(
            "SPIRV-Cross is revision {other}, this tool needs {SPIRV_CROSS_REVISION}"
        )),
        None => Err("the output of spirv-cross --revision is not understood".to_string()),
    }
}

fn os(text: &str) -> OsString {
    OsString::from(text)
}

fn path(p: &Path) -> OsString {
    p.as_os_str().to_os_string()
}

/// glslang: GLSL (stage from the file extension) to SPIR-V with Vulkan semantics.
#[must_use]
pub fn glslang_args(input: &Path, output: &Path) -> Vec<OsString> {
    vec![os("-V"), path(input), os("-o"), path(output)]
}

/// SPIRV-Cross: SPIR-V to desktop GLSL 450.
#[must_use]
pub fn cross_glsl_args(spirv: &Path, output: &Path) -> Vec<OsString> {
    vec![
        path(spirv),
        os("--version"),
        os("450"),
        os("--no-es"),
        os("--output"),
        path(output),
    ]
}

/// SPIRV-Cross: SPIR-V to the JSON reflection.
#[must_use]
pub fn cross_reflect_args(spirv: &Path, output: &Path) -> Vec<OsString> {
    vec![path(spirv), os("--reflect"), os("--output"), path(output)]
}

/// Replaces every `\r\n` with `\n`, so output is the same on every platform.
#[must_use]
pub fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Whether a `DUCK_REQUIRE_...` variable value asks for a failure instead of a skip: set and
/// neither empty nor `0`.
#[must_use]
pub fn is_required(value: Option<&str>) -> bool {
    value.is_some_and(|v| !v.is_empty() && v != "0")
}

#[cfg(test)]
mod tests;
