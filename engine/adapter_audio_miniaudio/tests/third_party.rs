//! The manifest that `build.rs` downloads from and `THIRD_PARTY.md` must name the same address and
//! the same hash: a change to one without the other fails here.

use adapter_runtime_libs::source::parse_source;

#[test]
fn the_source_manifest_agrees_with_third_party() {
    let manifest = include_str!("../source-file.txt");
    let register = include_str!("../../../THIRD_PARTY.md");
    let Ok(source) = parse_source(manifest) else {
        panic!("the manifest is not valid");
    };
    let row = register
        .lines()
        .find(|line| line.starts_with("| miniaudio"))
        .unwrap_or("");
    assert!(!row.is_empty(), "THIRD_PARTY.md has no miniaudio row");
    for (what, text) in [("address", &source.url), ("SHA-256", &source.sha256)] {
        assert!(
            row.contains(text.as_str()),
            "THIRD_PARTY.md names another {what}"
        );
    }
    assert!(
        row.contains("0.11.26") && source.url.contains("/0.11.26/"),
        "the version differs between the manifest and THIRD_PARTY.md"
    );
}
