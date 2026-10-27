import tempfile
import unittest
from pathlib import Path

import unsafe_budget as ub

ARCH = """
[layers]
base = 0
render = 2
"adapter_*" = 3
[rules]
exempt = ["skeleton-unsafe"]
"""

BUDGET = """
max_percent = 5

[[modules]]
path = "base/src/low.rs"
max_unsafe_lines = 3
"""

NO_MODULES = "max_percent = 5\n"

SAFE_BLOCK = "// SAFETY: the pointer is valid.\nlet _v = unsafe { *p };\n"


class Workspace:
    """A throwaway workspace: cargo-metadata-shaped dict plus real source files."""

    def __init__(self, test, crates):
        self.dir = Path(tempfile.mkdtemp())
        test.addCleanup(lambda: __import__("shutil").rmtree(self.dir, ignore_errors=True))
        self.packages = []
        for name, files in crates.items():
            for relative, text in files.items():
                path = self.dir / name / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text, encoding="utf-8")
            (self.dir / name).mkdir(exist_ok=True)
            self.packages.append({"name": name, "manifest_path": str(self.dir / name / "Cargo.toml")})
        self.metadata = {"packages": self.packages, "workspace_root": str(self.dir)}

    def run(self, budget=BUDGET, arch=ARCH):
        return ub.check(self.metadata, ub.load_budget(budget), ub.load_architecture(arch))


def rules(report):
    return sorted({finding.rule for finding in report.findings})


def filler(lines):
    return "fn f() {}\n" * lines


class Budget(unittest.TestCase):
    def test_a_crate_without_unsafe_is_clean(self):
        report = Workspace(self, {"base": {"src/lib.rs": filler(10)}}).run(NO_MODULES)
        self.assertEqual([], report.findings)
        self.assertEqual((0, 10), (report.unsafe_lines, report.code_lines))

    def test_an_unsafe_block_in_an_allowlisted_module_with_a_safety_comment_is_allowed(self):
        report = Workspace(self, {"base": {"src/low.rs": filler(100) + SAFE_BLOCK}}).run()
        self.assertEqual([], report.findings)
        self.assertEqual(1, report.unsafe_lines)

    def test_unsafe_outside_the_allowlist_is_refused(self):
        report = Workspace(self, {"base": {"src/other.rs": filler(100) + SAFE_BLOCK}}).run(NO_MODULES)
        self.assertEqual(["unsafe-outside-allowlist"], rules(report))

    def test_a_block_without_a_safety_comment_is_refused(self):
        report = Workspace(self, {"base": {"src/low.rs": filler(100) + "let _v = unsafe { *p };\n"}}).run()
        self.assertEqual(["missing-safety"], rules(report))

    def test_a_safety_comment_must_sit_directly_above(self):
        text = filler(100) + "// SAFETY: far away.\nlet a = 1;\nlet _v = unsafe { *p };\n"
        self.assertEqual(["missing-safety"], rules(Workspace(self, {"base": {"src/low.rs": text}}).run()))

    def test_a_multi_line_safety_comment_counts(self):
        text = filler(100) + "// SAFETY: the pointer\n// is valid.\nlet _v = unsafe { *p };\n"
        self.assertEqual([], Workspace(self, {"base": {"src/low.rs": text}}).run().findings)

    def test_an_unsafe_fn_needs_a_safety_section_in_its_docs(self):
        good = filler(100) + "/// Reads.\n///\n/// # Safety\n/// p must be valid.\nunsafe fn read(p: *const u8) -> u8 {\n    0\n}\n"
        bad = filler(100) + "/// Reads.\nunsafe fn read(p: *const u8) -> u8 {\n    0\n}\n"
        self.assertEqual([], Workspace(self, {"base": {"src/low.rs": good}}).run(BUDGET.replace("= 3", "= 9")).findings)
        found = Workspace(self, {"base": {"src/low.rs": bad}}).run(BUDGET.replace("= 3", "= 9"))
        self.assertEqual(["missing-safety"], rules(found))

    def test_an_unsafe_fn_inside_a_commented_unsafe_impl_needs_no_docs(self):
        text = filler(100) + "// SAFETY: forwards.\nunsafe impl G for T {\n    unsafe fn alloc(&self) -> u8 {\n        0\n    }\n}\n"
        report = Workspace(self, {"base": {"src/low.rs": text}}).run(BUDGET.replace("= 3", "= 9"))
        self.assertEqual([], report.findings)

    def test_an_unsafe_impl_needs_a_safety_comment(self):
        bad = filler(100) + "unsafe impl Send for T {}\n"
        self.assertEqual(["missing-safety"], rules(Workspace(self, {"base": {"src/low.rs": bad}}).run()))

    def test_a_module_over_its_budget_is_refused(self):
        text = filler(200) + SAFE_BLOCK * 4
        self.assertEqual(["module-budget"], rules(Workspace(self, {"base": {"src/low.rs": text}}).run()))

    def test_a_block_spanning_lines_counts_every_line(self):
        text = filler(100) + "// SAFETY: ok.\nunsafe {\n    if a {\n        b();\n    }\n}\n"
        report = Workspace(self, {"base": {"src/low.rs": text}}).run(BUDGET.replace("= 3", "= 9"))
        self.assertEqual(5, report.unsafe_lines)

    def test_the_ratio_over_the_limit_is_refused(self):
        text = filler(10) + SAFE_BLOCK
        report = Workspace(self, {"base": {"src/low.rs": text}}).run(BUDGET.replace("= 3", "= 99"))
        self.assertEqual(["unsafe-ratio"], rules(report))

    def test_the_ratio_is_taken_over_all_layer_0_to_2_crates(self):
        ws = Workspace(self, {"base": {"src/low.rs": filler(100) + SAFE_BLOCK}, "render": {"src/lib.rs": filler(100)}})
        self.assertEqual([], ws.run().findings)
        self.assertEqual(200, ws.run().code_lines - ws.run().unsafe_lines)

    def test_comments_strings_and_the_lint_name_are_not_unsafe(self):
        text = filler(100) + '// unsafe here\nlet s = "unsafe { }";\n#![expect(unsafe_code, reason = "x")]\n'
        report = Workspace(self, {"base": {"src/other.rs": text}}).run(NO_MODULES)
        self.assertEqual([], report.findings)
        self.assertEqual(0, report.unsafe_lines)

    def test_layer_3_and_exempt_crates_are_not_checked(self):
        ws = Workspace(self, {"adapter_gl": {"src/lib.rs": SAFE_BLOCK}, "skeleton-unsafe": {"src/lib.rs": SAFE_BLOCK}})
        report = ws.run(NO_MODULES)
        self.assertEqual([], report.findings)
        self.assertEqual(0, report.code_lines)

    def test_a_crate_without_a_layer_is_refused(self):
        report = Workspace(self, {"mystery": {"src/lib.rs": filler(3)}}).run(NO_MODULES)
        self.assertEqual(["unmapped"], rules(report))

    def test_an_allowlisted_file_that_does_not_exist_is_refused(self):
        report = Workspace(self, {"base": {"src/lib.rs": filler(3)}}).run()
        self.assertEqual(["stale-allowlist"], rules(report))

    def test_the_budget_needs_a_ratio(self):
        with self.assertRaises(ValueError):
            ub.load_budget('[[modules]]\npath = "a.rs"\nmax_unsafe_lines = 1\n')

    def test_the_ratio_must_be_a_finite_percentage(self):
        for value in ("nan", "inf", "-1", "101"):
            with self.assertRaises(ValueError, msg=value):
                ub.load_budget(f"max_percent = {value}\n")

    def test_a_path_listed_twice_is_refused(self):
        entry = '[[modules]]\npath = "a.rs"\nmax_unsafe_lines = 1\n'
        with self.assertRaises(ValueError):
            ub.load_budget("max_percent = 5\n" + entry + entry)

    def test_a_module_entry_needs_a_path_and_a_budget(self):
        with self.assertRaises(ValueError):
            ub.load_budget("max_percent = 5\n[[modules]]\npath = 'a.rs'\n")


class RealRepository(unittest.TestCase):
    def test_the_real_workspace_passes_the_real_budget(self):
        repo = Path(__file__).resolve().parents[2]
        code = ub.main(["--config", str(repo / "gates" / "unsafe-budget.toml"),
                        "--architecture", str(repo / "gates" / "architecture.toml"), "--repo", str(repo)],
                       print_line=lambda line: None)
        self.assertEqual(0, code)


if __name__ == "__main__":
    unittest.main()
