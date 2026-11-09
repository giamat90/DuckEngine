//! Tests of the single-file source fetch (a C header the build compiles). Nothing here touches the
//! network: the download is replaced by a fake that counts its calls and writes the bytes the test
//! chose.

use crate::manifest::ManifestError;
use crate::obtain::{ObtainError, Tools};
use crate::sha256::sha256_hex;
use crate::source::{SourceFile, obtain_source, parse_source, verify_source};
use std::cell::Cell;
use std::io;
use std::path::{Path, PathBuf};

const HEADER: &[u8] = b"/* pretend this is miniaudio.h */";

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("duck-source-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(std::fs::create_dir_all(&dir).is_ok());
    dir
}

fn manifest_text() -> String {
    format!(
        "# a comment\nfile = miniaudio.h\nurl = https://example.org/miniaudio.h\nsha256 = {}\noverride = DUCK_X\n",
        sha256_hex(HEADER)
    )
}

fn source() -> SourceFile {
    SourceFile {
        file: "miniaudio.h".to_owned(),
        url: "https://example.org/miniaudio.h".to_owned(),
        sha256: sha256_hex(HEADER),
        override_variable: None,
    }
}

struct Fake {
    bytes: &'static [u8],
    downloads: Cell<u32>,
    fail: bool,
}

impl Fake {
    fn good() -> Self {
        Self {
            bytes: HEADER,
            downloads: Cell::new(0),
            fail: false,
        }
    }
}

impl Tools for Fake {
    fn download(&self, _url: &str, dest: &Path) -> io::Result<()> {
        self.downloads.set(self.downloads.get() + 1);
        if self.fail {
            return Err(io::Error::other("no network"));
        }
        std::fs::write(dest, self.bytes)
    }

    fn unpack(&self, _archive: &Path, _member: &str, _dir: &Path) -> io::Result<()> {
        Err(io::Error::other("a plain file is never unpacked"))
    }
}

// ---------------------------------------------------------------- manifest

#[test]
fn a_good_source_manifest_is_read() {
    let parsed = parse_source(&manifest_text());
    assert_eq!(
        parsed,
        Ok(SourceFile {
            override_variable: Some("DUCK_X".to_owned()),
            ..source()
        })
    );
}

#[test]
fn the_override_is_optional() {
    let text = manifest_text().replace("override = DUCK_X\n", "");
    assert_eq!(parse_source(&text), Ok(source()));
}

#[test]
fn a_source_manifest_is_refused_when_it_is_wrong() {
    let text = manifest_text();
    assert!(matches!(
        parse_source(&text.replace("file = miniaudio.h\n", "")),
        Err(ManifestError::Missing(_))
    ));
    assert_eq!(
        parse_source(&text.replace("https://", "http://")),
        Err(ManifestError::NotHttps)
    );
    assert!(matches!(
        parse_source(&text.replace(&sha256_hex(HEADER), "ABC")),
        Err(ManifestError::BadHash(_))
    ));
    assert!(matches!(
        parse_source(&text.replace("file = miniaudio.h", "file = ../miniaudio.h")),
        Err(ManifestError::BadPath(_))
    ));
    assert!(matches!(
        parse_source(&text.replace("file = miniaudio.h", "file = sub/miniaudio.h")),
        Err(ManifestError::BadPath(_))
    ));
    for odd in [".", "a:stream", "./x"] {
        assert!(
            matches!(
                parse_source(&text.replace("file = miniaudio.h", &format!("file = {odd}"))),
                Err(ManifestError::BadPath(_))
            ),
            "{odd}"
        );
    }
    assert!(matches!(
        parse_source(&format!("{text}archive_sha256 = {}\n", sha256_hex(HEADER))),
        Err(ManifestError::UnknownKey(_))
    ));
    assert!(matches!(
        parse_source(&format!("{text}file = again.h\n")),
        Err(ManifestError::Duplicate(_))
    ));
    assert!(matches!(
        parse_source("nothing here"),
        Err(ManifestError::BadLine(_))
    ));
}

// ---------------------------------------------------------------- obtain

#[test]
fn the_file_is_downloaded_checked_and_kept() {
    let cache = scratch("fetch");
    let fake = Fake::good();
    let path = obtain_source(&source(), &cache, &fake, false);
    let Ok(path) = path else {
        panic!("a good download is accepted");
    };
    assert_eq!(path, cache.join(sha256_hex(HEADER)).join("miniaudio.h"));
    assert_eq!(std::fs::read(&path).ok().as_deref(), Some(HEADER));
    assert_eq!(fake.downloads.get(), 1);
}

#[test]
fn a_cached_file_is_not_downloaded_again() {
    let cache = scratch("cached");
    let fake = Fake::good();
    assert!(obtain_source(&source(), &cache, &fake, false).is_ok());
    assert!(obtain_source(&source(), &cache, &fake, false).is_ok());
    assert_eq!(fake.downloads.get(), 1);
}

#[test]
fn a_damaged_cached_file_is_replaced() {
    let cache = scratch("damaged");
    let fake = Fake::good();
    let Ok(path) = obtain_source(&source(), &cache, &fake, false) else {
        panic!("a good download is accepted");
    };
    assert!(std::fs::write(&path, b"tampered").is_ok());
    assert!(obtain_source(&source(), &cache, &fake, false).is_ok());
    assert_eq!(std::fs::read(&path).ok().as_deref(), Some(HEADER));
    assert_eq!(fake.downloads.get(), 2);
}

#[test]
fn offline_uses_the_cache_and_nothing_else() {
    let cache = scratch("offline");
    let fake = Fake::good();
    assert_eq!(
        obtain_source(&source(), &cache, &fake, true),
        Err(ObtainError::Offline)
    );
    assert_eq!(fake.downloads.get(), 0);
    assert!(obtain_source(&source(), &cache, &fake, false).is_ok());
    assert!(obtain_source(&source(), &cache, &fake, true).is_ok());
    assert_eq!(fake.downloads.get(), 1);
}

#[test]
fn a_file_with_another_hash_is_discarded() {
    let cache = scratch("wronghash");
    let fake = Fake {
        bytes: b"something else",
        ..Fake::good()
    };
    let result = obtain_source(&source(), &cache, &fake, false);
    assert!(matches!(result, Err(ObtainError::SourceHash { .. })));
    assert!(!cache.join(sha256_hex(HEADER)).exists());
    let leftovers = std::fs::read_dir(&cache).map(Iterator::count).ok();
    assert_eq!(leftovers, Some(0), "nothing is left in the cache");
}

#[test]
fn a_failed_download_is_an_error_and_leaves_nothing() {
    let cache = scratch("nonetwork");
    let fake = Fake {
        fail: true,
        ..Fake::good()
    };
    let result = obtain_source(&source(), &cache, &fake, false);
    assert!(matches!(result, Err(ObtainError::Tool(_))));
    assert_eq!(std::fs::read_dir(&cache).map(Iterator::count).ok(), Some(0));
}

#[test]
fn a_file_of_the_users_own_is_checked_against_the_same_hash() {
    let dir = scratch("verify");
    let good = dir.join("good.h");
    let bad = dir.join("bad.h");
    assert!(std::fs::write(&good, HEADER).is_ok());
    assert!(std::fs::write(&bad, b"other").is_ok());
    assert_eq!(verify_source(&good, &sha256_hex(HEADER)), Ok(()));
    assert!(matches!(
        verify_source(&bad, &sha256_hex(HEADER)),
        Err(ObtainError::SourceHash { .. })
    ));
    assert!(matches!(
        verify_source(&dir.join("missing.h"), &sha256_hex(HEADER)),
        Err(ObtainError::Tool(_))
    ));
}

#[test]
fn the_new_error_has_a_message() {
    let error = ObtainError::SourceHash {
        expected: "a".to_owned(),
        found: "b".to_owned(),
    };
    assert!(error.to_string().contains('b'));
}
