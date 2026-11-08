//! The manifest that `build.rs` downloads from and `THIRD_PARTY.md` must name the same address and
//! the same hashes: a change to one without the other fails here.

use adapter_runtime_libs::manifest::parse;

#[test]
fn the_runtime_manifest_agrees_with_third_party() {
    let manifest = include_str!("../runtime-lib.txt");
    let register = include_str!("../../../THIRD_PARTY.md");
    let Ok(lib) = parse(manifest) else {
        panic!("the manifest does not parse");
    };
    for hash in [&lib.archive_sha256, &lib.file_sha256] {
        assert!(
            register.contains(hash.as_str()),
            "THIRD_PARTY.md lacks {hash}"
        );
    }
    assert!(
        register.contains(&lib.file),
        "THIRD_PARTY.md lacks {}",
        lib.file
    );
    assert!(
        lib.url.ends_with("glfw-3.4.bin.WIN64.zip") && register.contains("glfw-3.4.bin.WIN64.zip"),
        "the archive named in the manifest is not the one in THIRD_PARTY.md"
    );
}
