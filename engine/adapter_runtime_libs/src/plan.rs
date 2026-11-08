//! What a build script does, as one testable function: [`run`] turns a manifest and the build
//! environment into the copies made, the files to watch and the warnings to print. It never fails:
//! a library that cannot be fetched is a warning that says what to do.

use crate::manifest::{Library, parse};
use crate::obtain::{Tools, obtain, place};
use std::path::{Path, PathBuf};

/// The environment variable that overrides the cache folder.
pub const CACHE_VARIABLE: &str = "DUCK_RUNTIME_LIBS_CACHE";
/// The environment variable that forbids downloading (a non-empty value other than `0`).
pub const OFFLINE_VARIABLE: &str = "DUCK_RUNTIME_LIBS_OFFLINE";

/// What a build script reports to cargo.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Problems, each one a line for `cargo:warning=`.
    pub warnings: Vec<String>,
    /// The copies made.
    pub placed: Vec<PathBuf>,
    /// Files whose change or absence should run the build script again.
    pub watch: Vec<PathBuf>,
}

/// The folders executables of this build land in, from the `OUT_DIR` of a build script
/// (`<target>/<profile>/build/<crate>-<hash>/out`): the profile folder (programs), `deps` (test
/// programs) and `examples`. `None` when `out_dir` is not shaped like that.
#[must_use]
pub fn output_dirs(out_dir: &Path) -> Option<Vec<PathBuf>> {
    let profile = out_dir.ancestors().nth(3)?;
    if profile.file_name().is_none() || out_dir.ancestors().nth(2)?.file_name()? != "build" {
        return None;
    }
    Some(vec![
        profile.to_path_buf(),
        profile.join("deps"),
        profile.join("examples"),
    ])
}

fn non_empty(lookup: &dyn Fn(&str) -> Option<String>, name: &str) -> Option<String> {
    lookup(name).filter(|value| !value.is_empty())
}

/// The folder downloaded libraries are kept in, so that `cargo clean` does not download them
/// again: `DUCK_RUNTIME_LIBS_CACHE`, else the user's cache folder, else the build folder.
#[must_use]
pub fn cache_root(
    lookup: &dyn Fn(&str) -> Option<String>,
    out_dir: &Path,
    target_os: &str,
) -> PathBuf {
    if let Some(folder) = non_empty(lookup, CACHE_VARIABLE) {
        return PathBuf::from(folder);
    }
    let user = if target_os == "windows" {
        non_empty(lookup, "LOCALAPPDATA").map(|base| Path::new(&base).join("DuckEngine"))
    } else {
        non_empty(lookup, "XDG_CACHE_HOME")
            .map(|base| Path::new(&base).join("duckengine"))
            .or_else(|| {
                non_empty(lookup, "HOME")
                    .map(|home| Path::new(&home).join(".cache").join("duckengine"))
            })
    };
    user.map_or_else(
        || out_dir.join("runtime-libs"),
        |base| base.join("runtime-libs"),
    )
}

fn offline(lookup: &dyn Fn(&str) -> Option<String>) -> bool {
    non_empty(lookup, OFFLINE_VARIABLE).is_some_and(|value| value != "0")
}

fn advice(lib: &Library) -> String {
    let own = lib
        .override_variable
        .as_ref()
        .map_or_else(String::new, |name| {
            format!(" or set {name} to its full path")
        });
    format!(
        "put {} next to the program yourself{own} (see THIRD_PARTY.md for the official download and its SHA-256)",
        lib.file
    )
}

fn warn(lib: &Library, problem: &str) -> Outcome {
    Outcome {
        warnings: vec![format!(
            "could not bring {} next to the program: {problem}; {}",
            lib.file,
            advice(lib)
        )],
        ..Outcome::default()
    }
}

/// Brings the library of `manifest_text` next to the executables of the build whose `OUT_DIR` is
/// `out_dir`. Does nothing for another operating system than the manifest's, or when the user's
/// own library is named by the manifest's override variable.
#[must_use]
pub fn run(
    manifest_text: &str,
    target_os: &str,
    out_dir: &Path,
    lookup: &dyn Fn(&str) -> Option<String>,
    tools: &dyn Tools,
) -> Outcome {
    let lib = match parse(manifest_text) {
        Ok(lib) => lib,
        Err(error) => {
            return Outcome {
                warnings: vec![format!(
                    "the runtime library manifest is not valid: {error}"
                )],
                ..Outcome::default()
            };
        }
    };
    if lib.os != target_os {
        return Outcome::default();
    }
    if lib
        .override_variable
        .as_deref()
        .is_some_and(|name| non_empty(lookup, name).is_some())
    {
        return Outcome::default();
    }
    let Some(dirs) = output_dirs(out_dir) else {
        return warn(&lib, "the build folder is not where it was expected");
    };
    // The cache lives on the machine that builds, not on the target.
    let cache = cache_root(lookup, out_dir, std::env::consts::OS);
    let watch = dirs
        .first()
        .map(|profile| profile.join(&lib.file))
        .into_iter()
        .collect();
    let source = std::fs::create_dir_all(&cache)
        .map_err(|e| format!("the cache folder cannot be made: {e}"))
        .and_then(|()| obtain(&lib, &cache, tools, offline(lookup)).map_err(|e| e.to_string()));
    match source {
        // Nothing is watched: a missing file would make cargo run the script (and the download)
        // again on every build. The manifest and the variables are still watched by build.rs.
        Err(problem) => warn(&lib, &problem),
        Ok(file) => match place(&file, &lib.file, &dirs) {
            Ok(placed) => Outcome {
                placed,
                watch,
                ..Outcome::default()
            },
            Err(error) => Outcome {
                watch,
                ..warn(
                    &lib,
                    &format!("the copy failed ({error}); is a program using it?"),
                )
            },
        },
    }
}
