"""Tests for the change classifier (card 0c-1).

Test list (architect-review-surface.md section 1, highest class wins, unsure rounds up):
  paths    protected path -> 3; contracts/ and the classifier itself -> 3 (also on delete/rename);
           Cargo.toml, build.rs, Cargo.lock, vendored dirs, jobs/, adapters/, base/alloc|simd|math*,
           baselines -> 2; module src/ and tests/, benches, docs and wiki -> 1; unknown path -> 2.
  content  in .rs files, an added `unsafe`, atomic, intrinsic or `pub` item -> 2; `pub(crate)` -> 1;
           tests/ files are exempt from the `pub` rule; removed lines are not scanned.
  combine  highest class wins; no changes -> 1; the reasons name the files behind the class.
  git      added lines per file, both names of a rename, deletes, bad revision is an error.
  cli      prints `class=N` plus reasons, exit 0; an error prints FAIL and exits 1.
"""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

import classify_change as cc

PROTECTED = ["gates/", "CLAUDE.md", "THIRD_PARTY.md", "tools/classify-change/"]
GIT_ENV = {
    **os.environ,
    "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.com",
}


def path_class(path):
    return cc.classify_path(path, PROTECTED)[0]


class PathClass3(unittest.TestCase):
    def test_protected_entries_are_class_3(self):
        for path in ("gates/gates.toml", "CLAUDE.md", "THIRD_PARTY.md"):
            with self.subTest(path=path):
                self.assertEqual(path_class(path), 3)

    def test_contracts_directories_are_class_3_at_any_depth(self):
        self.assertEqual(path_class("engine/window/contracts/window_contract.rs"), 3)
        self.assertEqual(path_class("contracts/x.rs"), 3)

    def test_a_sibling_that_only_looks_like_contracts_is_not(self):
        self.assertNotEqual(path_class("engine/a/src/contracts_helper.rs"), 3)


class PathClass2(unittest.TestCase):
    def test_dependency_and_build_files_are_class_2(self):
        for path in ("Cargo.toml", "engine/a/Cargo.toml", "Cargo.lock", "engine/a/build.rs"):
            with self.subTest(path=path):
                self.assertEqual(path_class(path), 2)

    def test_vendored_directories_are_class_2(self):
        self.assertEqual(path_class("third_party/glfw/x.c"), 2)
        self.assertEqual(path_class("vendor/x/lib.rs"), 2)

    def test_concurrency_memory_and_simd_areas_are_class_2(self):
        for path in ("engine/jobs/src/pool.rs", "engine/base/src/alloc.rs", "engine/base/src/alloc/arena.rs",
                     "engine/base/src/simd.rs", "engine/base/src/simd/avx2.rs", "engine/base/src/math.rs",
                     "engine/base/src/math/sin.rs"):
            with self.subTest(path=path):
                self.assertEqual(path_class(path), 2)

    def test_adapters_are_class_2(self):
        self.assertEqual(path_class("engine/adapters/glfw/src/lib.rs"), 2)

    def test_benchmark_baselines_are_class_2(self):
        self.assertEqual(path_class("baselines/skeleton.json"), 2)
        self.assertEqual(path_class("engine/a/benches/baseline.json"), 2)

    def test_an_unknown_path_rounds_up_to_2(self):
        self.assertEqual(path_class("some-new-top-level/file.txt"), 2)

    def test_names_that_merely_start_with_a_trigger_word_are_class_1(self):
        self.assertEqual(path_class("engine/a/src/mathematics_notes.rs"), 1)


class PathClass1(unittest.TestCase):
    def test_module_sources_and_tests_are_class_1(self):
        for path in ("engine/skeleton/src/util.rs", "engine/skeleton/tests/it.rs", "engine/skeleton/benches/b.rs"):
            with self.subTest(path=path):
                self.assertEqual(path_class(path), 1)

    def test_docs_wiki_reviews_and_root_markdown_are_class_1(self):
        for path in ("wiki/workflow/roadmap.md", "docs/gates/x.md", "reviews/0c-1.md", "START-HERE.md"):
            with self.subTest(path=path):
                self.assertEqual(path_class(path), 1)

    def test_class_3_wins_over_the_class_1_directory_it_sits_in(self):
        self.assertEqual(path_class("tools/classify-change/classify_change.py"), 3)

    def test_each_path_has_a_reason(self):
        self.assertTrue(cc.classify_path("engine/a/src/lib.rs", PROTECTED)[1])


class Content(unittest.TestCase):
    def content_class(self, path, lines):
        return cc.classify_content(path, lines)[0]

    def test_added_unsafe_is_class_2(self):
        self.assertEqual(self.content_class("engine/a/src/x.rs", ["    unsafe { f() }"]), 2)
        self.assertEqual(self.content_class("engine/a/src/x.rs", ["unsafe impl Send for X {}"]), 2)

    def test_atomics_and_intrinsics_are_class_2(self):
        for line in ("use std::sync::atomic::AtomicU32;", "let x = AtomicBool::new(false);",
                     "use core::arch::x86_64::_mm_add_ps;", "use std::arch::aarch64::*;", "std::sync::Mutex<u8>"):
            with self.subTest(line=line):
                self.assertEqual(self.content_class("engine/a/src/x.rs", [line]), 2)

    def test_a_new_public_item_is_class_2(self):
        for line in ("pub fn f() {}", "    pub struct S;", "pub const N: u32 = 1;", "pub mod m;", "pub use a::b;"):
            with self.subTest(line=line):
                self.assertEqual(self.content_class("engine/a/src/x.rs", [line]), 2)

    def test_restricted_visibility_is_not_public_api(self):
        for line in ("pub(crate) fn f() {}", "pub(super) struct S;", "pub ( crate ) fn g() {}"):
            with self.subTest(line=line):
                self.assertEqual(self.content_class("engine/a/src/x.rs", [line]), 1)

    def test_tests_and_benches_may_declare_public_items(self):
        self.assertEqual(self.content_class("engine/a/tests/it.rs", ["pub fn helper() {}"]), 1)
        self.assertEqual(self.content_class("engine/a/benches/b.rs", ["pub fn helper() {}"]), 1)

    def test_unsafe_in_a_test_file_is_still_class_2(self):
        self.assertEqual(self.content_class("engine/a/tests/it.rs", ["unsafe { f() }"]), 2)

    def test_words_that_only_contain_a_trigger_are_class_1(self):
        self.assertEqual(self.content_class("engine/a/src/x.rs", ["let public_key = 1; // not_unsafe_at_all"]), 1)

    def test_non_rust_files_are_not_scanned(self):
        self.assertEqual(self.content_class("wiki/page.md", ["pub fn f() {}", "unsafe"]), 1)

    def test_plain_code_is_class_1(self):
        self.assertEqual(self.content_class("engine/a/src/x.rs", ["let x = 1 + 2;"]), 1)


class ReviewFindings(unittest.TestCase):
    """Regressions for the independent review of 0c-1 (reviews/0c-1.md)."""

    def content_class(self, path, lines, removed=()):
        return cc.classify_content(path, lines, list(removed))[0]

    def test_f2_a_public_item_after_an_attribute_or_in_an_impl_is_class_2(self):
        for line in ("#[inline] pub fn f() {}", "#[derive(Debug)] pub struct S;", "impl S { pub fn f() {} }"):
            with self.subTest(line=line):
                self.assertEqual(self.content_class("engine/a/src/x.rs", [line]), 2)

    def test_f2_a_word_ending_in_pub_is_not_a_public_item(self):
        self.assertEqual(self.content_class("engine/a/src/x.rs", ["let republic = 1; let a::pub_x = 2;"]), 1)

    def test_f3_removing_or_changing_a_public_item_is_class_2(self):
        self.assertEqual(self.content_class("engine/a/src/x.rs", [], ["pub fn old() {}"]), 2)
        self.assertEqual(self.content_class("engine/a/src/x.rs", [], ["#[macro_export]"]), 2)

    def test_f3_removing_private_code_is_class_1(self):
        self.assertEqual(self.content_class("engine/a/src/x.rs", [], ["let a = 1;"]), 1)

    def test_f3_macro_export_and_macro_rules_are_class_2(self):
        for line in ("#[macro_export]", "macro_rules! m { () => {} }"):
            with self.subTest(line=line):
                self.assertEqual(self.content_class("engine/a/src/x.rs", [line]), 2)

    def test_f3_a_module_root_is_class_2_because_trait_and_enum_items_are_not_parsed(self):
        self.assertEqual(path_class("engine/a/src/lib.rs"), 2)
        self.assertEqual(path_class("engine/a/src/window/mod.rs"), 2)

    def test_f4_base_areas_match_by_prefix(self):
        for path in ("engine/base/src/allocator.rs", "engine/base/src/mathx.rs", "engine/base/src/simdlane.rs"):
            with self.subTest(path=path):
                self.assertEqual(path_class(path), 2)

    def test_f5_case_variants_of_protected_paths_are_class_3(self):
        for path in ("claude.md", "Third_Party.md", "GATES/gates.toml", "Tools/Classify-Change/x.py",
                     "engine/a/CONTRACTS/c.rs"):
            with self.subTest(path=path):
                self.assertEqual(path_class(path), 3)

    def test_f5_windows_aliases_of_a_path_are_class_3(self):
        for path in ("gates\\gates.toml", "CLAUDE.md.", "CLAUDE.md ", "CLAUDE.md::$DATA", "CLAUDE~1.MD", "gates./x"):
            with self.subTest(path=path):
                self.assertEqual(path_class(path), 3)

    def test_f7_governance_files_are_class_3_at_any_depth(self):
        for path in ("clippy.toml", "engine/a/clippy.toml", "engine/a/deny.toml", "engine/a/rust-toolchain.toml"):
            with self.subTest(path=path):
                self.assertEqual(path_class(path), 3)

    def test_f8_unsafe_adjacent_constructs_are_class_2(self):
        for line in ("use std::{arch::asm};", "let c = UnsafeCell::new(1);", "static mut X: u32 = 0;",
                     'extern "C" fn f() {}', "#[no_mangle]", "#[target_feature(enable = \"avx2\")]",
                     "use core::intrinsics::x;"):
            with self.subTest(line=line):
                self.assertEqual(self.content_class("engine/a/src/x.rs", [line]), 2)

    def test_f9_only_known_text_types_are_routine(self):
        self.assertEqual(path_class("wiki/page.md"), 1)
        self.assertEqual(path_class("reviews/x.txt"), 1)
        self.assertEqual(path_class("wiki/x.py"), 2)
        self.assertEqual(path_class("engine/a/src/build.sh"), 2)
        self.assertEqual(path_class("engine/a/src/shader.json"), 2)

    def test_f10_the_pub_exemption_is_only_for_the_crate_level_tests_and_benches(self):
        self.assertEqual(self.content_class("engine/a/src/tests/h.rs", ["pub fn helper() {}"]), 2)

    def test_f11_a_submodule_bump_is_class_2(self):
        self.assertEqual(self.content_class("docs/sub", ["Subproject commit 0123abcd"]), 2)

    def test_f1_a_change_with_no_readable_lines_is_class_2(self):
        self.assertEqual(cc.classify({"engine/a/src/x.rs": cc.Change("M", [], [], opaque=True)}, PROTECTED).level, 2)

    def test_f12_a_malformed_protected_entry_is_an_error(self):
        for text in ("./gates/\n", "gates\\x/\n", "gates/ # comment\n"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                cc.load_protected(text)


class Combine(unittest.TestCase):
    def test_the_highest_class_wins(self):
        changes = {
            "engine/a/src/x.rs": cc.Change("M", ["let x = 1;"]),
            "engine/a/Cargo.toml": cc.Change("M", ["serde = 1"]),
            "gates/gates.toml": cc.Change("M", []),
        }
        result = cc.classify(changes, PROTECTED)
        self.assertEqual(result.level, 3)

    def test_no_changes_is_class_1(self):
        self.assertEqual(cc.classify({}, PROTECTED).level, 1)

    def test_content_can_raise_a_class_1_path_to_2(self):
        changes = {"engine/a/src/x.rs": cc.Change("M", ["pub fn f() {}"])}
        self.assertEqual(cc.classify(changes, PROTECTED).level, 2)

    def test_reasons_name_the_files_behind_the_class_only(self):
        changes = {
            "engine/a/src/x.rs": cc.Change("M", ["let x = 1;"]),
            "engine/a/Cargo.toml": cc.Change("M", []),
        }
        result = cc.classify(changes, PROTECTED)
        self.assertEqual(len(result.reasons), 1)
        self.assertIn("engine/a/Cargo.toml", result.reasons[0])
        self.assertNotIn("engine/a/src/x.rs", result.reasons[0])

    def test_an_empty_protected_list_is_an_error(self):
        with self.assertRaises(ValueError):
            cc.load_protected("# nothing\n")


class Git(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.path = Path(self._tmp.name)
        self.git("init", "-q", "-b", "main")
        self.git("config", "commit.gpgsign", "false")

    def git(self, *args):
        done = subprocess.run(["git", *args], cwd=self.path, env=GIT_ENV, capture_output=True, check=True)
        return done.stdout.decode().strip()

    def commit(self, name, text="x\n"):
        target = self.path / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", name)

    def test_added_lines_are_collected_per_file(self):
        self.commit("base.txt")
        self.git("checkout", "-q", "-b", "topic")
        self.commit("engine/a/src/x.rs", "let a = 1;\npub fn f() {}\n")
        found = cc.changes(self.path, "main", "topic")
        self.assertEqual(found["engine/a/src/x.rs"].added, ["let a = 1;", "pub fn f() {}"])

    def test_removed_lines_are_not_scanned(self):
        self.commit("engine/a/src/x.rs", "pub fn old() {}\nlet a = 1;\n")
        self.git("checkout", "-q", "-b", "topic")
        self.commit("engine/a/src/x.rs", "let a = 1;\n")
        found = cc.changes(self.path, "main", "topic")
        self.assertEqual(found["engine/a/src/x.rs"].added, [])

    def test_a_plus_plus_line_inside_the_text_is_content_not_a_header(self):
        self.commit("base.txt")
        self.git("checkout", "-q", "-b", "topic")
        self.commit("engine/a/src/x.rs", "++ unsafe { f() }\n")
        found = cc.changes(self.path, "main", "topic")
        self.assertEqual(found["engine/a/src/x.rs"].added, ["++ unsafe { f() }"])

    def test_a_deleted_protected_file_is_listed_and_is_class_3(self):
        self.commit("gates/gates.toml")
        self.git("checkout", "-q", "-b", "topic")
        self.git("rm", "-q", "gates/gates.toml")
        self.git("commit", "-q", "-m", "rm")
        found = cc.changes(self.path, "main", "topic")
        self.assertEqual(cc.classify(found, PROTECTED).level, 3)

    def test_a_renamed_protected_file_counts_under_both_names(self):
        self.commit("gates/gates.toml", "a\nb\nc\nd\n")
        self.git("checkout", "-q", "-b", "topic")
        (self.path / "docs").mkdir()
        self.git("mv", "gates/gates.toml", "docs/moved.toml")
        self.git("commit", "-q", "-m", "move")
        found = cc.changes(self.path, "main", "topic")
        self.assertEqual(sorted(found), ["docs/moved.toml", "gates/gates.toml"])
        self.assertEqual(cc.classify(found, PROTECTED).level, 3)

    def test_a_binary_change_is_listed_and_rounds_up(self):
        self.commit("base.txt")
        self.git("checkout", "-q", "-b", "topic")
        (self.path / "engine").mkdir()
        (self.path / "engine" / "blob.bin").write_bytes(b"\x00\x01\x02")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "bin")
        found = cc.changes(self.path, "main", "topic")
        self.assertIn("engine/blob.bin", found)
        self.assertEqual(cc.classify(found, PROTECTED).level, 2)

    def test_an_unknown_revision_is_an_error(self):
        self.commit("a.txt")
        with self.assertRaises(cc.ClassifyError):
            cc.changes(self.path, "main", "missing")

    def test_f11_a_revision_that_looks_like_an_option_is_an_error_not_an_option(self):
        self.commit("a.txt")
        for revision in ("--output=x.txt", "-p"):
            with self.subTest(revision=revision), self.assertRaises(cc.ClassifyError):
                cc.changes(self.path, "main", revision)
        self.assertFalse((self.path / "x.txt").exists())

    def test_f1_a_rust_file_with_a_nul_byte_is_still_scanned(self):
        self.commit("base.txt")
        self.git("checkout", "-q", "-b", "topic")
        self.commit("engine/x/src/m.rs", "fn a() { unsafe {} } // \0\n")
        found = cc.changes(self.path, "main", "topic")
        self.assertEqual(cc.classify(found, PROTECTED).level, 2)

    def test_f1_a_gitattributes_no_diff_rule_does_not_hide_the_lines(self):
        self.commit(".gitattributes", "*.rs -diff\n")
        self.git("checkout", "-q", "-b", "topic")
        self.commit("engine/x/src/m.rs", "fn a() { unsafe {} }\n")
        found = cc.changes(self.path, "main", "topic")
        self.assertEqual(cc.classify(found, PROTECTED).level, 2)

    def test_f1_a_mode_only_change_has_no_readable_lines_and_is_class_2(self):
        self.commit("engine/x/src/m.rs", "fn a() {}\n")
        self.git("checkout", "-q", "-b", "topic")
        self.git("update-index", "--chmod=+x", "engine/x/src/m.rs")
        self.git("commit", "-q", "-m", "mode")
        found = cc.changes(self.path, "main", "topic")
        self.assertTrue(found["engine/x/src/m.rs"].opaque)
        self.assertEqual(cc.classify(found, PROTECTED).level, 2)

    def test_f1_a_readable_change_and_a_delete_are_not_opaque(self):
        self.commit("engine/x/src/a.rs", "fn a() {}\n")
        self.commit("engine/x/src/b.rs", "fn b() {}\n")
        self.git("checkout", "-q", "-b", "topic")
        self.commit("engine/x/src/a.rs", "fn a() { 1; }\n")
        self.git("rm", "-q", "engine/x/src/b.rs")
        self.git("commit", "-q", "-m", "rm")
        found = cc.changes(self.path, "main", "topic")
        self.assertFalse(found["engine/x/src/a.rs"].opaque)
        self.assertFalse(found["engine/x/src/b.rs"].opaque)

    def test_f3_removed_lines_are_collected(self):
        self.commit("engine/a/src/x.rs", "pub fn old() {}\nlet a = 1;\n")
        self.git("checkout", "-q", "-b", "topic")
        self.commit("engine/a/src/x.rs", "let a = 1;\n")
        found = cc.changes(self.path, "main", "topic")
        self.assertEqual(found["engine/a/src/x.rs"].removed, ["pub fn old() {}"])
        self.assertEqual(cc.classify(found, PROTECTED).level, 2)


class Main(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.path = Path(self._tmp.name)
        self.paths = self.path / "protected.txt"
        self.paths.write_text("gates/\n", encoding="utf-8")
        self.git("init", "-q", "-b", "main")
        self.git("config", "commit.gpgsign", "false")
        (self.path / "a.md").write_text("x\n", encoding="utf-8")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "base")
        self.git("checkout", "-q", "-b", "topic")

    def git(self, *args):
        subprocess.run(["git", *args], cwd=self.path, env=GIT_ENV, capture_output=True, check=True)

    def run_main(self, head):
        lines = []
        args = ["--repo", str(self.path), "--base", "main", "--head", head, "--paths", str(self.paths)]
        return cc.main(args, lines.append), lines

    def test_prints_the_class_and_the_reasons(self):
        (self.path / "gates").mkdir()
        (self.path / "gates" / "g.toml").write_text("x\n", encoding="utf-8")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "g")
        code, lines = self.run_main("topic")
        self.assertEqual(code, 0)
        self.assertEqual(lines[0], "class=3")
        self.assertIn("gates/g.toml", lines[1])

    def test_a_docs_only_change_is_class_1(self):
        (self.path / "wiki").mkdir()
        (self.path / "wiki" / "p.md").write_text("x\n", encoding="utf-8")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "p")
        code, lines = self.run_main("topic")
        self.assertEqual((code, lines[0]), (0, "class=1"))

    def test_f11_an_empty_diff_is_an_error_not_class_1(self):
        code, lines = self.run_main("topic")
        self.assertEqual(code, 1)
        self.assertTrue(lines[0].startswith("FAIL"))
        self.assertFalse(any(line.startswith("class=") for line in lines))

    def test_an_error_fails_and_never_prints_a_class(self):
        code, lines = self.run_main("missing")
        self.assertEqual(code, 1)
        self.assertTrue(lines[0].startswith("FAIL"))
        self.assertFalse(any(line.startswith("class=") for line in lines))


class RealProtectedList(unittest.TestCase):
    """The real list plus the classifier's own rules must cover what the digest calls Class 3."""

    ROOT = Path(__file__).resolve().parents[2]

    def test_the_classifier_itself_and_the_governance_files_are_class_3(self):
        listed = cc.load_protected((self.ROOT / "gates" / "protected-paths.txt").read_text(encoding="utf-8"))
        for path in ("tools/classify-change/classify_change.py", "gates/gates.toml", ".github/workflows/ci.yml",
                     "wiki/rules-digest.md", "CLAUDE.md", "docs/adr/0001-x.md", "THIRD_PARTY.md",
                     ".GITHUB/workflows/ci.yml", "Wiki/Rules-Digest.md", "Docs/ADR/x.md", ".claude/skills/x/SKILL.md",
                     ".githooks/pre-commit"):
            with self.subTest(path=path):
                self.assertEqual(cc.classify_path(path, listed)[0], 3)


if __name__ == "__main__":
    unittest.main()
