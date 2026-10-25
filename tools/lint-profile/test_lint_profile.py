"""Tests for the clean-code lint profile (card 0b-3).

Two kinds of test:
- profile tests: clippy.toml carries the numbers of gates/gates.toml, and every
  crate's lint table enables the clean-code lints;
- seeded tests: throwaway crates, linted with the repo's own clippy.toml and
  lint table, must be flagged just above each limit and stay clean at it.

Limits that Clippy cannot express (cyclomatic complexity, the "warn" tier of
function parameters, duplication, magic numbers) are listed in
docs/gates/clean-code-lint-profile.md and are not tested here.
"""

import json
import os
import re
import subprocess
import tempfile
import tomllib
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
CARGO_TOML = REPO / "Cargo.toml"
CLIPPY_TOML = REPO / "clippy.toml"
GATES_TOML = REPO / "gates" / "gates.toml"
TOOLCHAIN_TOML = REPO / "rust-toolchain.toml"
UNSAFE_CRATE_TOML = REPO / "engine" / "skeleton-unsafe" / "Cargo.toml"

# clippy.toml key -> gates.toml [clean_code] key
CLIPPY_KEYS = {
    "too-many-lines-threshold": "function_length_warn",
    "cognitive-complexity-threshold": "cognitive_fail",
    "too-many-arguments-threshold": "parameters_fail",
    "excessive-nesting-threshold": "nesting_max",
}

# The lints the profile must turn on, and the limit each one polices.
CLEAN_CODE_LINTS = ("too_many_lines", "cognitive_complexity", "too_many_arguments", "excessive_nesting")
LINT_LEVELS = ("warn", "deny")
# The lints that make a waiver visible: no `allow` attribute, and every `expect` names its reason.
WAIVER_LINTS = ("allow_attributes", "allow_attributes_without_reason")

CARGO = os.environ.get("CARGO", "cargo")


def read_toml(path):
    return tomllib.loads(path.read_text(encoding="utf-8"))


def gates_limit(key):
    return read_toml(GATES_TOML)["clean_code"][key]


def clippy_lints(manifest, workspace):
    """The [lints.clippy] table of a manifest (or [workspace.lints.clippy])."""
    data = read_toml(manifest)
    table = data["workspace"]["lints"] if workspace else data["lints"]
    return table["clippy"]


def level_of(entry):
    return entry["level"] if isinstance(entry, dict) else entry


def lint_table_text(manifest, header):
    """Raw text of one lint table, so a seed crate gets exactly the repo's lines."""
    text = manifest.read_text(encoding="utf-8")
    match = re.search(rf"^\[{re.escape(header)}\]\n(.*?)(?=^\[|\Z)", text, re.S | re.M)
    return match.group(1)


def seed_manifest():
    return (
        '[package]\nname = "seed"\nversion = "0.0.0"\nedition = "2024"\npublish = false\n\n'
        "[workspace]\n\n"
        f"[lints.rust]\n{lint_table_text(CARGO_TOML, 'workspace.lints.rust')}\n"
        f"[lints.clippy]\n{lint_table_text(CARGO_TOML, 'workspace.lints.clippy')}"
    )


def clippy_codes(source, target_dir):
    """Run Clippy with the repo's profile on one source file; return the lint codes it raised."""
    with tempfile.TemporaryDirectory() as tmp:
        crate = Path(tmp)
        (crate / "src").mkdir()
        (crate / "Cargo.toml").write_text(seed_manifest(), encoding="utf-8")
        (crate / "src" / "lib.rs").write_text(source, encoding="utf-8")
        for name in (CLIPPY_TOML, TOOLCHAIN_TOML):
            (crate / name.name).write_text(name.read_text(encoding="utf-8"), encoding="utf-8")
        env = dict(os.environ, CARGO_TARGET_DIR=str(target_dir))
        done = subprocess.run(
            [CARGO, "clippy", "--message-format=json", "--quiet"],
            cwd=crate, env=env, capture_output=True, text=True, check=False,
        )
    codes = set()
    for line in done.stdout.splitlines():
        message = json.loads(line) if line.startswith("{") else {}
        body = message.get("message") or {}
        code = (body.get("code") or {}).get("code")
        if body.get("level") == "error" and not (code or "").startswith("clippy::"):
            raise AssertionError(f"seed crate did not compile:\n{done.stdout}")
        if code:
            codes.add(code)
    if done.returncode != 0 and not codes:
        raise AssertionError(f"clippy did not run:\n{done.stderr}")
    return codes


def function_with_lines(body_lines):
    """A function with exactly `body_lines` lines between its braces (Clippy's length measure)."""
    body = "    x = x.wrapping_add(1);\n" * (body_lines - 1)
    return f"#[must_use]\npub fn long(mut x: u32) -> u32 {{\n{body}    x\n}}\n"


def function_with_params(count):
    names = ", ".join(f"p{i}: u32" for i in range(count))
    total = " + ".join(f"p{i}" for i in range(count))
    return f"#[must_use]\npub fn wide({names}) -> u32 {{\n    {total}\n}}\n"


def function_nested(levels):
    """`levels` nested loops inside a function. Clippy counts the function body as level 1."""
    opens = "".join("    " * (i + 1) + "for _ in 0..a {\n" for i in range(levels))
    closes = "".join("    " * (levels - i) + "}\n" for i in range(levels))
    return (
        f"#[must_use]\npub fn deep(a: u32) -> u32 {{\n    let mut t = 0;\n{opens}"
        f"{'    ' * (levels + 1)}t += 1;\n{closes}    t\n}}\n"
    )


def function_tangled(repeats):
    """Repeated loop-with-branches blocks: cognitive complexity grows with each repeat."""
    arms = "".join(
        f"    for i in 0..n {{\n        if i % {k + 2} == 0 {{\n            t += i;\n"
        f"        }} else if i > {k} {{\n            t += 2;\n        }}\n    }}\n"
        for k in range(repeats)
    )
    return f"#[must_use]\npub fn tangled(n: u32) -> u32 {{\n    let mut t = 0;\n{arms}    t\n}}\n"


class ProfileMatchesGates(unittest.TestCase):
    def test_clippy_toml_exists(self):
        self.assertTrue(CLIPPY_TOML.is_file())

    def test_every_threshold_equals_the_gate_number(self):
        clippy = read_toml(CLIPPY_TOML)
        for clippy_key, gates_key in CLIPPY_KEYS.items():
            with self.subTest(key=clippy_key):
                self.assertEqual(clippy.get(clippy_key), gates_limit(gates_key))

    def test_clippy_toml_has_no_other_threshold(self):
        self.assertEqual(set(read_toml(CLIPPY_TOML)), set(CLIPPY_KEYS))


class LintTables(unittest.TestCase):
    def check_table(self, lints):
        for lint in CLEAN_CODE_LINTS:
            with self.subTest(lint=lint):
                self.assertIn(lint, lints)
                self.assertIn(level_of(lints[lint]), LINT_LEVELS)

    def test_workspace_table_enables_the_clean_code_lints(self):
        self.check_table(clippy_lints(CARGO_TOML, workspace=True))

    def test_workspace_table_denies_unreasoned_and_plain_allow_attributes(self):
        lints = clippy_lints(CARGO_TOML, workspace=True)
        for lint in WAIVER_LINTS:
            with self.subTest(lint=lint):
                self.assertEqual("deny", level_of(lints.get(lint)))

    def test_unsafe_crate_repeats_the_clean_code_lints(self):
        self.check_table(clippy_lints(UNSAFE_CRATE_TOML, workspace=False))

    def test_unsafe_crate_repeats_every_workspace_lint(self):
        workspace = clippy_lints(CARGO_TOML, workspace=True)
        local = clippy_lints(UNSAFE_CRATE_TOML, workspace=False)
        for lint, entry in workspace.items():
            with self.subTest(lint=lint):
                self.assertEqual(local.get(lint), entry)


class EveryCrateIsLinted(unittest.TestCase):
    def test_each_member_inherits_the_workspace_lints_or_repeats_them(self):
        members = read_toml(CARGO_TOML)["workspace"]["members"]
        for member in members:
            with self.subTest(member=member):
                manifest = REPO / member / "Cargo.toml"
                lints = read_toml(manifest).get("lints", {})
                self.assertTrue(lints.get("workspace") or "clippy" in lints)
                if not lints.get("workspace"):
                    self.assertEqual(
                        {k: lints["clippy"].get(k) for k in CLEAN_CODE_LINTS},
                        {k: clippy_lints(CARGO_TOML, workspace=True)[k] for k in CLEAN_CODE_LINTS},
                    )


class SeededViolations(unittest.TestCase):
    """Cognitive complexity is tested coarsely (far under, far over): Clippy's score is not
    a count we can build to exactly 20, so the exact number is pinned by the profile tests."""

    @classmethod
    def setUpClass(cls):
        cls._target = tempfile.TemporaryDirectory()
        cls.target = Path(cls._target.name)

    @classmethod
    def tearDownClass(cls):
        cls._target.cleanup()

    def codes(self, source):
        return clippy_codes(source, self.target)

    def test_clean_function_raises_nothing(self):
        self.assertEqual(self.codes("#[must_use]\npub fn id(x: u32) -> u32 {\n    x\n}\n"), set())

    def test_function_at_the_length_limit_is_clean(self):
        limit = gates_limit("function_length_warn")
        self.assertNotIn("clippy::too_many_lines", self.codes(function_with_lines(limit)))

    def test_function_over_the_length_limit_is_flagged(self):
        limit = gates_limit("function_length_warn")
        self.assertIn("clippy::too_many_lines", self.codes(function_with_lines(limit + 1)))

    def test_parameters_at_the_fail_limit_are_clean(self):
        limit = gates_limit("parameters_fail")
        self.assertNotIn("clippy::too_many_arguments", self.codes(function_with_params(limit)))

    def test_parameters_over_the_fail_limit_are_flagged(self):
        limit = gates_limit("parameters_fail")
        self.assertIn("clippy::too_many_arguments", self.codes(function_with_params(limit + 1)))

    # Clippy counts the function body as nesting level 1, so nesting_max = 3
    # allows 2 nested loops inside a function and flags the third.
    def test_nesting_at_the_limit_is_clean(self):
        levels = gates_limit("nesting_max") - 1
        self.assertNotIn("clippy::excessive_nesting", self.codes(function_nested(levels)))

    def test_nesting_over_the_limit_is_flagged(self):
        levels = gates_limit("nesting_max")
        self.assertIn("clippy::excessive_nesting", self.codes(function_nested(levels)))

    def test_an_allow_attribute_is_flagged(self):
        self.assertIn("clippy::allow_attributes", self.codes("#[allow(dead_code)]\nfn unused() {}\n"))

    def test_an_expect_without_a_reason_is_flagged(self):
        self.assertIn("clippy::allow_attributes_without_reason", self.codes("#[expect(dead_code)]\nfn unused() {}\n"))

    def test_an_expect_with_a_reason_is_clean(self):
        self.assertEqual(set(), self.codes('#[expect(dead_code, reason = "seeded")]\nfn unused() {}\n'))

    def test_small_tangle_is_clean(self):
        self.assertNotIn("clippy::cognitive_complexity", self.codes(function_tangled(2)))

    def test_large_tangle_is_flagged(self):
        self.assertIn("clippy::cognitive_complexity", self.codes(function_tangled(8)))


if __name__ == "__main__":
    unittest.main()
