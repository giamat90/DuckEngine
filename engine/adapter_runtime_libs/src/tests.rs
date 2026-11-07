//! Tests of the build helper. Nothing here touches the network: the download and the unpacking are
//! replaced by a fake that counts its calls and writes bytes the test chose.

use crate::manifest::{Library, ManifestError, parse};
use crate::obtain::{ObtainError, Tools, obtain, place};
use crate::plan::{cache_root, output_dirs, run};
use crate::sha256::{Sha256, file_sha256_hex, sha256_hex};
use std::cell::Cell;
use std::io;
use std::path::{Path, PathBuf};

const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const MILLION_A: &str = "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0";
const TWO_BLOCKS: &str = "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1";

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("duck-runtime-libs-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(std::fs::create_dir_all(&dir).is_ok());
    dir
}

// ---------------------------------------------------------------- sha256

#[test]
fn the_hash_matches_the_standard_test_vectors() {
    assert_eq!(sha256_hex(b""), EMPTY);
    assert_eq!(sha256_hex(b"abc"), ABC);
    assert_eq!(
        sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        TWO_BLOCKS
    );
    assert_eq!(sha256_hex(&vec![b'a'; 1_000_000]), MILLION_A);
}

#[test]
fn feeding_the_bytes_in_pieces_gives_the_same_hash() {
    let data: Vec<u8> = (0..1000_u32)
        .map(|n| u8::try_from(n % 251).unwrap_or(0))
        .collect();
    let whole = sha256_hex(&data);
    for piece in [1_usize, 7, 63, 64, 65, 500] {
        let mut hash = Sha256::new();
        data.chunks(piece).for_each(|part| hash.update(part));
        let hex = crate::sha256::hex(&hash.finish());
        assert_eq!(hex, whole, "piece {piece}");
    }
}

#[test]
fn a_file_is_hashed_like_its_bytes() {
    let dir = scratch("filehash");
    let path = dir.join("f.bin");
    assert!(std::fs::write(&path, b"abc").is_ok());
    assert_eq!(file_sha256_hex(&path).ok().as_deref(), Some(ABC));
    assert!(file_sha256_hex(&dir.join("missing")).is_err());
}

// ---------------------------------------------------------------- manifest

fn manifest_text() -> String {
    format!(
        "# a comment\nfile = lib.dll\nurl = https://example.org/pack.zip\narchive_sha256 = {ABC}\nmember = pack/x64/lib.dll\nfile_sha256 = {EMPTY}\nos = windows\noverride = MY_LIB\n"
    )
}

#[test]
fn a_manifest_names_the_file_the_archive_and_the_hashes() {
    let lib = parse(&manifest_text());
    assert_eq!(
        lib,
        Ok(Library {
            file: "lib.dll".to_owned(),
            url: "https://example.org/pack.zip".to_owned(),
            archive_sha256: ABC.to_owned(),
            member: "pack/x64/lib.dll".to_owned(),
            file_sha256: EMPTY.to_owned(),
            os: "windows".to_owned(),
            override_variable: Some("MY_LIB".to_owned()),
        })
    );
}

#[test]
fn the_override_variable_is_optional_and_blank_lines_are_ignored() {
    let text = manifest_text().replace("override = MY_LIB\n", "\n\n");
    assert_eq!(parse(&text).map(|l| l.override_variable), Ok(None));
}

fn without(key: &str) -> String {
    manifest_text()
        .lines()
        .filter(|line| !line.starts_with(key))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn every_required_key_is_required() {
    for key in [
        "file",
        "url",
        "archive_sha256",
        "member",
        "file_sha256",
        "os",
    ] {
        assert_eq!(
            parse(&without(key)),
            Err(ManifestError::Missing(key.to_owned())),
            "{key}"
        );
    }
}

#[test]
fn a_manifest_that_could_be_abused_is_refused() {
    let bad = |from: &str, to: &str| parse(&manifest_text().replace(from, to));
    assert_eq!(
        bad("https://example.org", "http://example.org"),
        Err(ManifestError::NotHttps)
    );
    assert!(matches!(bad(ABC, "ABC"), Err(ManifestError::BadHash(_))));
    assert!(matches!(
        bad(EMPTY, &EMPTY.to_uppercase()),
        Err(ManifestError::BadHash(_))
    ));
    assert_eq!(
        bad("file = lib.dll", "file = ../lib.dll"),
        Err(ManifestError::BadPath("file".to_owned()))
    );
    assert_eq!(
        bad("member = pack/x64/lib.dll", "member = ../x/lib.dll"),
        Err(ManifestError::BadPath("member".to_owned()))
    );
    assert_eq!(
        bad("member = pack/x64/lib.dll", "member = /abs/lib.dll"),
        Err(ManifestError::BadPath("member".to_owned()))
    );
    assert!(matches!(
        parse("no equals sign here"),
        Err(ManifestError::BadLine(_))
    ));
    assert!(matches!(
        parse(&format!(
            "{}\nurl = https://example.org/b.zip",
            manifest_text()
        )),
        Err(ManifestError::Duplicate(_))
    ));
    assert!(matches!(
        parse(&format!("{}\nsurprise = 1", manifest_text())),
        Err(ManifestError::UnknownKey(_))
    ));
}

#[test]
fn every_manifest_error_has_a_message() {
    let errors = [
        ManifestError::Missing("url".to_owned()),
        ManifestError::NotHttps,
        ManifestError::BadHash("file_sha256".to_owned()),
        ManifestError::BadPath("member".to_owned()),
        ManifestError::BadLine("x".to_owned()),
        ManifestError::Duplicate("url".to_owned()),
        ManifestError::UnknownKey("zzz".to_owned()),
    ];
    for error in errors {
        assert!(!error.to_string().is_empty());
    }
}

// ---------------------------------------------------------------- obtain

/// The bytes of the fake archive and of the file inside it, and the library they make.
const ARCHIVE: &[u8] = b"pretend this is a zip";
const FILE: &[u8] = b"pretend this is a dll";

fn library() -> Library {
    Library {
        file: "lib.dll".to_owned(),
        url: "https://example.org/pack.zip".to_owned(),
        archive_sha256: sha256_hex(ARCHIVE),
        member: "pack/x64/lib.dll".to_owned(),
        file_sha256: sha256_hex(FILE),
        os: "windows".to_owned(),
        override_variable: None,
    }
}

struct Fake {
    archive: &'static [u8],
    file: &'static [u8],
    downloads: Cell<u32>,
    unpacks: Cell<u32>,
    fail_download: bool,
    leave_member_out: bool,
}

impl Fake {
    fn good() -> Self {
        Self {
            archive: ARCHIVE,
            file: FILE,
            downloads: Cell::new(0),
            unpacks: Cell::new(0),
            fail_download: false,
            leave_member_out: false,
        }
    }
}

impl Tools for Fake {
    fn download(&self, _url: &str, dest: &Path) -> io::Result<()> {
        self.downloads.set(self.downloads.get() + 1);
        if self.fail_download {
            return Err(io::Error::other("no network"));
        }
        std::fs::write(dest, self.archive)
    }

    fn unpack(&self, _archive: &Path, member: &str, dir: &Path) -> io::Result<()> {
        self.unpacks.set(self.unpacks.get() + 1);
        if self.leave_member_out {
            return Ok(());
        }
        let target = dir.join(member);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(target, self.file)
    }
}

fn files_under(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let (folders, files): (Vec<_>, Vec<_>) = entries.flatten().partition(|e| e.path().is_dir());
    files.len()
        + folders
            .iter()
            .map(|e| files_under(&e.path()))
            .sum::<usize>()
}

#[test]
fn a_library_is_downloaded_unpacked_checked_and_cached() {
    let cache = scratch("obtain-ok");
    let tools = Fake::good();
    let lib = library();
    let path = obtain(&lib, &cache, &tools, false);
    let path = path.unwrap_or_else(|_| unreachable!());
    assert_eq!(std::fs::read(&path).ok().as_deref(), Some(FILE));
    assert!(path.starts_with(&cache));
    assert!(path.ends_with("lib.dll"));
    assert_eq!((tools.downloads.get(), tools.unpacks.get()), (1, 1));
    // only the kept file is left in the cache: no archive, no work folder
    assert_eq!(files_under(&cache), 1);
}

#[test]
fn a_cached_library_is_used_without_the_network() {
    let cache = scratch("obtain-cached");
    let lib = library();
    let first = Fake::good();
    assert!(obtain(&lib, &cache, &first, false).is_ok());
    let second = Fake::good();
    assert!(obtain(&lib, &cache, &second, false).is_ok());
    assert_eq!((second.downloads.get(), second.unpacks.get()), (0, 0));
    // offline works from the cache too
    assert!(obtain(&lib, &cache, &second, true).is_ok());
}

#[test]
fn a_damaged_cache_entry_is_replaced() {
    let cache = scratch("obtain-damaged");
    let lib = library();
    let tools = Fake::good();
    let path = obtain(&lib, &cache, &tools, false).unwrap_or_else(|_| unreachable!());
    assert!(std::fs::write(&path, b"tampered").is_ok());
    let again = Fake::good();
    let path = obtain(&lib, &cache, &again, false).unwrap_or_else(|_| unreachable!());
    assert_eq!(std::fs::read(&path).ok().as_deref(), Some(FILE));
    assert_eq!(again.downloads.get(), 1);
}

#[test]
fn offline_without_a_cache_entry_is_an_error_and_downloads_nothing() {
    let cache = scratch("obtain-offline");
    let tools = Fake::good();
    assert_eq!(
        obtain(&library(), &cache, &tools, true),
        Err(ObtainError::Offline)
    );
    assert_eq!(tools.downloads.get(), 0);
}

#[test]
fn an_archive_with_the_wrong_hash_is_refused_and_leaves_nothing() {
    let cache = scratch("obtain-badarchive");
    let tools = Fake {
        archive: b"evil archive",
        ..Fake::good()
    };
    let result = obtain(&library(), &cache, &tools, false);
    assert!(
        matches!(result, Err(ObtainError::ArchiveHash { .. })),
        "{result:?}"
    );
    assert_eq!(
        tools.unpacks.get(),
        0,
        "an unchecked archive is never unpacked"
    );
    assert_eq!(files_under(&cache), 0);
}

#[test]
fn a_file_with_the_wrong_hash_is_refused_and_leaves_nothing() {
    let cache = scratch("obtain-badfile");
    let tools = Fake {
        file: b"evil dll",
        ..Fake::good()
    };
    let result = obtain(&library(), &cache, &tools, false);
    assert!(
        matches!(result, Err(ObtainError::FileHash { .. })),
        "{result:?}"
    );
    assert_eq!(files_under(&cache), 0);
}

#[test]
fn a_failed_download_and_a_missing_member_are_errors_that_leave_nothing() {
    let cache = scratch("obtain-fail");
    let down = Fake {
        fail_download: true,
        ..Fake::good()
    };
    assert!(matches!(
        obtain(&library(), &cache, &down, false),
        Err(ObtainError::Tool(_))
    ));
    let empty = Fake {
        leave_member_out: true,
        ..Fake::good()
    };
    assert!(matches!(
        obtain(&library(), &cache, &empty, false),
        Err(ObtainError::Tool(_) | ObtainError::MemberMissing)
    ));
    assert_eq!(files_under(&cache), 0);
}

#[test]
fn every_obtain_error_has_a_message_that_says_what_to_do() {
    let errors = [
        ObtainError::Offline,
        ObtainError::Tool("curl failed".to_owned()),
        ObtainError::MemberMissing,
        ObtainError::ArchiveHash {
            expected: "a".to_owned(),
            found: "b".to_owned(),
        },
        ObtainError::FileHash {
            expected: "a".to_owned(),
            found: "b".to_owned(),
        },
    ];
    for error in errors {
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn placing_copies_into_every_folder_once_and_repairs_a_changed_copy() {
    let root = scratch("place");
    let source = root.join("cache-lib.dll");
    assert!(std::fs::write(&source, FILE).is_ok());
    let (a, b, missing) = (root.join("a"), root.join("b"), root.join("not-there"));
    assert!(std::fs::create_dir_all(&a).is_ok() && std::fs::create_dir_all(&b).is_ok());
    let dirs = vec![a.clone(), b.clone(), missing.clone()];
    let placed = place(&source, "lib.dll", &dirs).unwrap_or_default();
    assert_eq!(placed, vec![a.join("lib.dll"), b.join("lib.dll")]);
    assert!(
        !missing.exists(),
        "a folder that does not exist is not made"
    );
    assert_eq!(std::fs::read(a.join("lib.dll")).ok().as_deref(), Some(FILE));
    // nothing to do the second time
    assert_eq!(
        place(&source, "lib.dll", &dirs).unwrap_or_default(),
        Vec::<PathBuf>::new()
    );
    assert!(std::fs::write(b.join("lib.dll"), b"changed").is_ok());
    assert_eq!(
        place(&source, "lib.dll", &dirs).unwrap_or_default(),
        vec![b.join("lib.dll")]
    );
}

// ---------------------------------------------------------------- plan

#[test]
fn the_output_folders_are_the_profile_folder_and_its_deps_and_examples() {
    let out = Path::new("T")
        .join("debug")
        .join("build")
        .join("crate-1234")
        .join("out");
    let dirs = output_dirs(&out).unwrap_or_default();
    let profile = Path::new("T").join("debug");
    assert_eq!(
        dirs,
        vec![
            profile.clone(),
            profile.join("deps"),
            profile.join("examples")
        ]
    );
    assert_eq!(output_dirs(Path::new("out")), None);
}

fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let owned: Vec<(String, String)> = pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect();
    move |name| {
        owned
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    }
}

#[test]
fn the_cache_is_the_one_named_by_the_variable_or_the_users_cache_or_the_build_folder() {
    let out = Path::new("O");
    assert_eq!(
        cache_root(&env(&[("DUCK_RUNTIME_LIBS_CACHE", "C")]), out, "windows"),
        PathBuf::from("C")
    );
    assert_eq!(
        cache_root(&env(&[("LOCALAPPDATA", "L")]), out, "windows"),
        Path::new("L").join("DuckEngine").join("runtime-libs")
    );
    assert_eq!(
        cache_root(&env(&[("XDG_CACHE_HOME", "X")]), out, "linux"),
        Path::new("X").join("duckengine").join("runtime-libs")
    );
    assert_eq!(
        cache_root(&env(&[("HOME", "H")]), out, "linux"),
        Path::new("H")
            .join(".cache")
            .join("duckengine")
            .join("runtime-libs")
    );
    assert_eq!(
        cache_root(&env(&[]), out, "windows"),
        out.join("runtime-libs")
    );
}

fn build_dirs(name: &str) -> (PathBuf, PathBuf) {
    let target = scratch(name);
    let profile = target.join("debug");
    let out = profile.join("build").join("c-1").join("out");
    for dir in [&out, &profile.join("deps"), &profile.join("examples")] {
        assert!(std::fs::create_dir_all(dir).is_ok());
    }
    (profile, out)
}

fn text_for_fake() -> String {
    let lib = library();
    format!(
        "file = {}\nurl = {}\narchive_sha256 = {}\nmember = {}\nfile_sha256 = {}\nos = windows\noverride = MY_LIB\n",
        lib.file, lib.url, lib.archive_sha256, lib.member, lib.file_sha256
    )
}

#[test]
fn a_windows_build_places_the_library_next_to_the_programs() {
    let (profile, out) = build_dirs("run-ok");
    let cache = scratch("run-ok-cache");
    let cache_text = cache.to_string_lossy().into_owned();
    let tools = Fake::good();
    let outcome = run(
        &text_for_fake(),
        "windows",
        &out,
        &env(&[("DUCK_RUNTIME_LIBS_CACHE", &cache_text)]),
        &tools,
    );
    assert_eq!(outcome.warnings, Vec::<String>::new());
    for dir in [&profile, &profile.join("deps"), &profile.join("examples")] {
        assert_eq!(
            std::fs::read(dir.join("lib.dll")).ok().as_deref(),
            Some(FILE)
        );
    }
    assert_eq!(outcome.watch, vec![profile.join("lib.dll")]);
    // the second build changes nothing and does not touch the network
    let again = Fake::good();
    let outcome = run(
        &text_for_fake(),
        "windows",
        &out,
        &env(&[("DUCK_RUNTIME_LIBS_CACHE", &cache_text)]),
        &again,
    );
    assert_eq!(outcome.warnings, Vec::<String>::new());
    assert_eq!(again.downloads.get(), 0);
}

#[test]
fn another_operating_system_or_an_override_variable_does_nothing() {
    let (profile, out) = build_dirs("run-skip");
    let tools = Fake::good();
    let linux = run(&text_for_fake(), "linux", &out, &env(&[]), &tools);
    assert_eq!(linux.warnings, Vec::<String>::new());
    let chosen = run(
        &text_for_fake(),
        "windows",
        &out,
        &env(&[("MY_LIB", "C:/x/lib.dll")]),
        &tools,
    );
    assert_eq!(chosen.warnings, Vec::<String>::new());
    assert_eq!(tools.downloads.get(), 0);
    assert!(!profile.join("lib.dll").exists());
}

#[test]
fn a_problem_is_a_warning_that_says_what_to_do_and_never_a_failure() {
    let (profile, out) = build_dirs("run-warn");
    let cache = scratch("run-warn-cache");
    let cache_text = cache.to_string_lossy().into_owned();
    let lookup = env(&[("DUCK_RUNTIME_LIBS_CACHE", &cache_text)]);
    // no network
    let down = Fake {
        fail_download: true,
        ..Fake::good()
    };
    let outcome = run(&text_for_fake(), "windows", &out, &lookup, &down);
    assert_eq!(outcome.warnings.len(), 1, "{:?}", outcome.warnings);
    assert!(outcome.warnings[0].contains("lib.dll") && outcome.warnings[0].contains("MY_LIB"));
    // offline asked for, nothing cached
    let offline = env(&[
        ("DUCK_RUNTIME_LIBS_CACHE", &cache_text),
        ("DUCK_RUNTIME_LIBS_OFFLINE", "1"),
    ]);
    assert_eq!(
        run(&text_for_fake(), "windows", &out, &offline, &down)
            .warnings
            .len(),
        1
    );
    // a manifest that does not parse
    let broken = run("nonsense", "windows", &out, &lookup, &down);
    assert_eq!(broken.warnings.len(), 1);
    // an output folder that is not inside a target folder
    let lost = run(
        &text_for_fake(),
        "windows",
        Path::new("out"),
        &lookup,
        &Fake::good(),
    );
    assert_eq!(lost.warnings.len(), 1);
    assert!(!profile.join("lib.dll").exists());
}

#[test]
fn offline_set_to_zero_or_empty_is_not_offline() {
    let (profile, out) = build_dirs("run-offline-zero");
    let cache = scratch("run-offline-zero-cache");
    let cache_text = cache.to_string_lossy().into_owned();
    for value in ["0", ""] {
        let lookup = env(&[
            ("DUCK_RUNTIME_LIBS_CACHE", &cache_text),
            ("DUCK_RUNTIME_LIBS_OFFLINE", value),
        ]);
        let outcome = run(&text_for_fake(), "windows", &out, &lookup, &Fake::good());
        assert_eq!(outcome.warnings, Vec::<String>::new(), "{value:?}");
        assert!(profile.join("lib.dll").exists());
    }
}
