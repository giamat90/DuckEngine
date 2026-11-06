"""Tests for the clean-code lint profile (card 0b-3).

Two kinds of test:
- profile tests: clippy.toml carries the numbers of gates/gates.toml (and so does every
  crate-level clippy.toml, card 4-19), and every crate's lint table enables the clean-code lints;
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
PHYSICS_CLIPPY_TOML = REPO / "engine" / "physics" / "clippy.toml"
ARCH_TOML = REPO / "gates" / "architecture.toml"
BAN_KEYS = ("disallowed-methods", "disallowed-types")

# Bans the physics crate file must keep (wiki/architecture/physics.md section 3): one or more per class.
REQUIRED_METHOD_BANS = ("f32::sin", "f32::powi", "f32::mul_add", "f64::sin", "std::time::Instant::now",
                        "std::time::SystemTime::now", "slice::sort_unstable", "slice::sort_unstable_by_key")
REQUIRED_TYPE_BANS = ("std::collections::HashMap", "std::collections::HashSet")
# Folders that never hold a tracked config file (hidden folders are skipped as well).
SKIPPED_DIRS = ("target", "node_modules")

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


def config_files(root=REPO):
    """Every clippy.toml and .clippy.toml under `root`, skipping hidden folders and build output."""
    found = []
    for folder, dirs, names in os.walk(root):
        dirs[:] = sorted(d for d in dirs if not d.startswith(".") and d not in SKIPPED_DIRS)
        found += [Path(folder) / n for n in sorted(names) if n.lower() in ("clippy.toml", ".clippy.toml")]
    return found


def threshold_problems(config):
    """The thresholds of a Clippy config that differ from gates.toml (a missing key counts as different)."""
    return [
        f"{key} is {config.get(key)!r}, gates.toml says {gates_limit(gates_key)}"
        for key, gates_key in CLIPPY_KEYS.items()
        if config.get(key) != gates_limit(gates_key)
    ]


def key_problems(config, extra=()):
    """Keys other than the four thresholds (and `extra`): a crate file may not add other settings,
    such as allow-*-in-tests or msrv, and no ban entry may carry allow-invalid."""
    problems = [f"unexpected key {key}" for key in config if key not in (*CLIPPY_KEYS, *extra)]
    for key in extra:
        problems += [f"{key} entry {e.get('path')} sets allow-invalid" for e in config.get(key, []) if e.get("allow-invalid")]
    return problems


def scan_names():
    """The method names the arch-check scan bans in physics (gates/architecture.toml), from its dot pattern."""
    for rule in read_toml(ARCH_TOML)["banned_use"]:
        if rule.get("crates") == ["physics"] and rule["pattern"].startswith("\\.\\s*(?:"):
            return set(re.search(r"\(\?:([^)]*)\)", rule["pattern"]).group(1).split("|"))
    raise AssertionError("the physics dot-call rule of the arch-check scan was not found")


def ban_problems(config):
    """Required bans missing from a physics crate config."""
    methods = {entry["path"] for entry in config.get("disallowed-methods", [])}
    types = {entry["path"] for entry in config.get("disallowed-types", [])}
    return [f"disallowed-methods lacks {path}" for path in REQUIRED_METHOD_BANS if path not in methods] + [
        f"disallowed-types lacks {path}" for path in REQUIRED_TYPE_BANS if path not in types
    ]


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


def clippy_codes(source, target_dir, config=CLIPPY_TOML, text=None):
    """The lint codes Clippy raised (see clippy_run)."""
    return clippy_run(source, target_dir, config, text)[0]


def clippy_run(source, target_dir, config=CLIPPY_TOML, text=None):
    """Run Clippy with the repo's profile on one source file; return (lint codes, message texts).
    `config` is the clippy.toml the seed crate gets (the root file or a crate-level one);
    `text` gives the content of that file directly."""
    with tempfile.TemporaryDirectory() as tmp:
        crate = Path(tmp)
        (crate / "src").mkdir()
        (crate / "Cargo.toml").write_text(seed_manifest(), encoding="utf-8")
        (crate / "src" / "lib.rs").write_text(source, encoding="utf-8")
        config_text = config.read_text(encoding="utf-8") if text is None else text
        (crate / "clippy.toml").write_text(config_text, encoding="utf-8")
        (crate / TOOLCHAIN_TOML.name).write_text(TOOLCHAIN_TOML.read_text(encoding="utf-8"), encoding="utf-8")
        env = dict(os.environ, CARGO_TARGET_DIR=str(target_dir))
        done = subprocess.run(
            [CARGO, "clippy", "--message-format=json", "--quiet"],
            cwd=crate, env=env, capture_output=True, text=True, check=False,
        )
    codes, messages = set(), []
    for line in done.stdout.splitlines():
        message = json.loads(line) if line.startswith("{") else {}
        body = message.get("message") or {}
        code = (body.get("code") or {}).get("code")
        if body.get("level") == "error" and not (code or "").startswith("clippy::"):
            raise AssertionError(f"seed crate did not compile:\n{done.stdout}")
        if code:
            codes.add(code)
        if body.get("message"):
            messages.append(body["message"])
    if done.returncode != 0 and not codes:
        raise AssertionError(f"clippy did not run:\n{done.stderr}")
    return codes, messages


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


class CrateLevelConfigs(unittest.TestCase):
    """Clippy reads one clippy.toml per crate and a crate-level file replaces the root file (card 4-19).
    So every clippy.toml (or .clippy.toml) in the tree must repeat the four thresholds of gates.toml,
    otherwise a crate file would silently relax a limit."""

    def test_every_config_in_the_tree_carries_the_gate_numbers(self):
        files = config_files()
        self.assertIn(CLIPPY_TOML, files)
        for path in files:
            with self.subTest(file=path.relative_to(REPO).as_posix()):
                self.assertEqual([], threshold_problems(read_toml(path)))

    def test_a_relaxed_threshold_is_reported(self):
        config = dict(read_toml(CLIPPY_TOML), **{"too-many-lines-threshold": 200})
        self.assertEqual(1, len(threshold_problems(config)))
        self.assertIn("too-many-lines-threshold", threshold_problems(config)[0])

    def test_a_missing_threshold_is_reported(self):
        config = read_toml(CLIPPY_TOML)
        del config["excessive-nesting-threshold"]
        self.assertIn("excessive-nesting-threshold", threshold_problems(config)[0])

    def test_a_tightened_threshold_is_reported_too(self):
        config = dict(read_toml(CLIPPY_TOML), **{"too-many-arguments-threshold": 2})
        self.assertEqual(1, len(threshold_problems(config)))

    def test_a_hidden_config_is_found_and_build_output_is_skipped(self):
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / "engine" / "x").mkdir(parents=True)
            (Path(tmp) / "engine" / "x" / ".clippy.toml").write_text("", encoding="utf-8")
            (Path(tmp) / "target" / "y").mkdir(parents=True)
            (Path(tmp) / "target" / "y" / "clippy.toml").write_text("", encoding="utf-8")
            found = [p.relative_to(tmp).as_posix() for p in config_files(Path(tmp))]
        self.assertEqual(["engine/x/.clippy.toml"], found)

    def test_a_physics_config_without_bans_is_reported(self):
        self.assertEqual([], ban_problems(read_toml(PHYSICS_CLIPPY_TOML)))
        self.assertEqual(len(REQUIRED_METHOD_BANS) + len(REQUIRED_TYPE_BANS), len(ban_problems(read_toml(CLIPPY_TOML))))
        config = read_toml(PHYSICS_CLIPPY_TOML)
        config["disallowed-methods"] = [e for e in config["disallowed-methods"] if e["path"] != "f32::sin"]
        self.assertIn("f32::sin", ban_problems(config)[0])

    def test_a_crate_config_may_not_add_other_settings(self):
        for path in config_files():
            extra = BAN_KEYS if path == PHYSICS_CLIPPY_TOML else ()
            with self.subTest(file=path.relative_to(REPO).as_posix()):
                self.assertEqual([], key_problems(read_toml(path), extra))

    def test_other_settings_and_allow_invalid_are_reported(self):
        config = dict(read_toml(PHYSICS_CLIPPY_TOML), msrv="1.0.0")
        self.assertEqual(["unexpected key msrv"], key_problems(config, BAN_KEYS))
        config = read_toml(PHYSICS_CLIPPY_TOML)
        config["disallowed-methods"][0]["allow-invalid"] = True
        self.assertIn("allow-invalid", key_problems(config, BAN_KEYS)[0])
        self.assertEqual(["unexpected key disallowed-methods"], key_problems({"disallowed-methods": []}))

    def test_float_bans_cover_f32_and_f64_alike_and_the_arch_check_names(self):
        paths = [e["path"] for e in read_toml(PHYSICS_CLIPPY_TOML)["disallowed-methods"]]
        banned = scan_names()
        for float_type in ("f32", "f64"):
            with self.subTest(float_type=float_type):
                prefix = f"{float_type}::"
                self.assertEqual(banned, {p[len(prefix):] for p in paths if p.startswith(prefix)})

    def test_every_ban_has_a_reason(self):
        config = read_toml(PHYSICS_CLIPPY_TOML)
        for entry in config["disallowed-methods"] + config["disallowed-types"]:
            with self.subTest(path=entry["path"]):
                self.assertTrue(entry.get("reason"))


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

    # Card 4-19: the physics crate file carries the bans. Clippy reads it as the only config of the crate.
    def physics_codes(self, source):
        return clippy_codes(source, self.target, PHYSICS_CLIPPY_TOML)

    def test_every_ban_path_resolves(self):
        # An unresolvable path is only a warning without a lint code; -D warnings would fail the build,
        # but on main there is no physics crate to show it, so the file is run through Clippy here.
        _, messages = clippy_run("pub const A: u32 = 1;\n", self.target, PHYSICS_CLIPPY_TOML)
        self.assertEqual([], [m for m in messages if "reachable" in m or "could not be resolved" in m])

    # Clippy warns about an unresolvable path only when the root is a module or type; a typo after a
    # primitive root (`f32::sinn`, `slice::sort_unstable_byy`) is silent and the entry just never fires.
    # So every such entry is seeded: each one must be reported on a use of the real method.
    def test_every_float_and_slice_ban_fires(self):
        entries = [e["path"] for e in read_toml(PHYSICS_CLIPPY_TOML)["disallowed-methods"]]
        floats = [p for p in entries if p.startswith(("f32::", "f64::"))]
        slices = [p for p in entries if p.startswith("slice::")]
        calls = "".join(f"    let _ = {p};\n" for p in floats)
        slice_calls = {
            "slice::sort_unstable": "v.sort_unstable();",
            "slice::sort_unstable_by": "v.sort_unstable_by(|a, b| a.cmp(b));",
            "slice::sort_unstable_by_key": "v.sort_unstable_by_key(|a| *a);",
            "slice::select_nth_unstable": "v.select_nth_unstable(0);",
            "slice::select_nth_unstable_by": "v.select_nth_unstable_by(0, |a, b| a.cmp(b));",
            "slice::select_nth_unstable_by_key": "v.select_nth_unstable_by_key(0, |a| *a);",
        }
        self.assertEqual(set(slice_calls), set(slices))
        source = (
            "pub fn floats() {\n" + calls + "}\n"
            "pub fn slices(v: &mut [u32]) {\n" + "".join(f"    {c}\n" for c in slice_calls.values()) + "}\n"
        )
        _, messages = clippy_run(source, self.target, PHYSICS_CLIPPY_TOML)
        for path in floats + slices:
            with self.subTest(ban=path):
                self.assertTrue(any(f"`{path}`" in m for m in messages))

    def test_physics_config_flags_each_class_of_ban(self):
        seeds = {
            "f32::sin": "pub fn f(x: f32) -> f32 {\n    x.sin()\n}\n",
            "f64::exp": "pub fn f(x: f64) -> f64 {\n    x.exp()\n}\n",
            "f32::mul_add": "pub fn f(x: f32) -> f32 {\n    x.mul_add(1.0, 2.0)\n}\n",
            "std::time::Instant::now": "pub fn f() {\n    let _ = std::time::Instant::now();\n}\n",
            "slice::sort_unstable": "pub fn f(v: &mut [u32]) {\n    v.sort_unstable();\n}\n",
        }
        for name, source in seeds.items():
            with self.subTest(ban=name):
                codes, messages = clippy_run(source, self.target, PHYSICS_CLIPPY_TOML)
                self.assertIn("clippy::disallowed_methods", codes)
                self.assertTrue(any(f"`{name}`" in m for m in messages), messages)
                self.assertNotIn("clippy::disallowed_methods", self.codes(source))

    def test_physics_config_flags_hash_collections_and_random_state(self):
        seeds = {
            "std::collections::HashMap": "pub fn f() -> usize {\n    std::collections::HashMap::<u32, u32>::new().len()\n}\n",
            "std::collections::HashSet": "pub fn f() -> usize {\n    std::collections::HashSet::<u32>::new().len()\n}\n",
            "std::hash::RandomState": "pub fn f() -> std::hash::RandomState {\n    std::hash::RandomState::new()\n}\n",
        }
        for name, source in seeds.items():
            with self.subTest(ban=name):
                codes, messages = clippy_run(source, self.target, PHYSICS_CLIPPY_TOML)
                self.assertIn("clippy::disallowed_types", codes)
                self.assertTrue(any(f"`{name}`" in m for m in messages), messages)

    def test_physics_config_allows_the_exact_float_operations(self):
        source = (
            "#[must_use]\npub fn f(x: f32) -> f32 {\n"
            "    x.sqrt().abs().floor().min(1.0) + x.ceil().max(0.0) + x.clamp(0.0, 1.0)\n}\n"
        )
        self.assertEqual(set(), self.physics_codes(source))

    def test_physics_config_keeps_the_clean_code_limits(self):
        limit = gates_limit("function_length_warn")
        self.assertNotIn("clippy::too_many_lines", self.physics_codes(function_with_lines(limit)))
        self.assertIn("clippy::too_many_lines", self.physics_codes(function_with_lines(limit + 1)))

    def test_a_crate_file_replaces_the_root_file(self):
        # Why the thresholds are repeated: a crate file without them falls back to Clippy's defaults.
        loose = "disallowed-methods = []\n"
        self.assertNotIn("clippy::too_many_lines", clippy_codes(function_with_lines(75), self.target, text=loose))

    def test_small_tangle_is_clean(self):
        self.assertNotIn("clippy::cognitive_complexity", self.codes(function_tangled(2)))

    def test_large_tangle_is_flagged(self):
        self.assertIn("clippy::cognitive_complexity", self.codes(function_tangled(8)))


if __name__ == "__main__":
    unittest.main()
