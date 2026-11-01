import subprocess
import tempfile
import unittest
from pathlib import Path

import swap_diff as sd

CONFIG = """
max_engine_game_files = 0
adapter = ["engine/adapter_*/**"]
composition_root = ["engine/app_*/**"]
ports = ["engine/platform/src/**"]
plumbing = ["Cargo.toml", "Cargo.lock"]
non_code = ["wiki/**", "reviews/**", "gates/**", "tools/**", ".github/**", "THIRD_PARTY.md"]
"""


def rules(text=CONFIG):
    return sd.load_rules(text)


def kinds(*paths, text=CONFIG):
    return {path: sd.classify(path, rules(text)) for path in paths}


class Classify(unittest.TestCase):
    def test_each_kind_of_file_lands_in_its_own_class(self):
        got = kinds("engine/adapter_window_glfw/src/lib.rs", "engine/app_runtime/src/main.rs", "engine/platform/src/contract.rs",
                    "Cargo.lock", "wiki/README.md", "engine/base/src/lib.rs", "samples/demo/src/main.rs")
        self.assertEqual(
            {"engine/adapter_window_glfw/src/lib.rs": "adapter", "engine/app_runtime/src/main.rs": "composition_root",
             "engine/platform/src/contract.rs": "ports", "Cargo.lock": "plumbing", "wiki/README.md": "non_code",
             "engine/base/src/lib.rs": "engine_game", "samples/demo/src/main.rs": "engine_game"}, got)

    def test_a_star_does_not_cross_a_slash_but_a_double_star_does(self):
        text = CONFIG.replace('plumbing = ["Cargo.toml", "Cargo.lock"]', 'plumbing = ["engine/*/Cargo.toml"]')
        got = kinds("engine/base/Cargo.toml", "engine/base/src/Cargo.toml", "engine/adapter_x/src/deep/file.rs", text=text)
        self.assertEqual(["plumbing", "engine_game", "adapter"], list(got.values()))

    def test_a_crate_name_that_merely_starts_like_an_adapter_is_not_one(self):
        self.assertEqual("engine_game", sd.classify("engine/adapters/src/lib.rs", rules()))

    def test_the_first_matching_class_in_the_fixed_order_wins(self):
        text = CONFIG.replace('non_code = [', 'non_code = ["engine/platform/**", ')
        self.assertEqual("ports", sd.classify("engine/platform/src/contract.rs", rules(text)))


class Report(unittest.TestCase):
    def test_a_range_with_no_adapter_or_composition_root_file_proves_nothing_and_fails(self):
        for files in ([], ["wiki/x.md"], ["engine/platform/src/contract.rs"]):
            report = sd.check(files, rules())
            self.assertTrue(report.vacuous, files)
            self.assertFalse(report.ok, files)

    def test_a_composition_root_file_alone_is_a_swap(self):
        self.assertTrue(sd.check(["engine/app_runtime/src/main.rs"], rules()).ok)

    def test_only_adapter_and_non_code_files_pass_with_zero_engine_game_files(self):
        report = sd.check(["engine/adapter_a/src/lib.rs", "wiki/x.md", "Cargo.lock"], rules())
        self.assertEqual((0, 0), (len(report.counts["engine_game"]), len(report.counts["ports"])))
        self.assertTrue(report.ok)

    def test_an_engine_file_fails_and_is_named(self):
        report = sd.check(["engine/adapter_a/src/lib.rs", "engine/base/src/lib.rs"], rules())
        self.assertFalse(report.ok)
        self.assertEqual(["engine/base/src/lib.rs"], report.counts["engine_game"])

    def test_port_files_do_not_fail_but_are_reported_as_a_port_revision(self):
        report = sd.check(["engine/adapter_a/src/lib.rs", "engine/platform/src/contract.rs", "engine/platform/src/lib.rs"], rules())
        self.assertTrue(report.ok)
        self.assertEqual(2, len(report.counts["ports"]))
        self.assertTrue(report.port_revision)

    def test_no_port_file_means_no_port_revision(self):
        self.assertFalse(sd.check(["engine/adapter_a/src/lib.rs", "wiki/x.md"], rules()).port_revision)

    def test_the_allowed_number_of_engine_game_files_comes_from_the_config(self):
        text = CONFIG.replace("max_engine_game_files = 0", "max_engine_game_files = 1")
        adapter = "engine/adapter_a/src/lib.rs"
        self.assertTrue(sd.check([adapter, "engine/base/src/lib.rs"], rules(text)).ok)
        self.assertFalse(sd.check([adapter, "engine/base/src/lib.rs", "engine/frame/src/lib.rs"], rules(text)).ok)

    def test_regex_characters_in_a_glob_are_literal(self):
        text = CONFIG.replace('plumbing = ["Cargo.toml", "Cargo.lock"]', 'plumbing = ["a+b.toml"]')
        got = kinds("a+b.toml", "aab.toml", "a+bxtoml", text=text)
        self.assertEqual(["plumbing", "engine_game", "engine_game"], list(got.values()))


class Config(unittest.TestCase):
    def test_a_missing_limit_is_refused(self):
        with self.assertRaises(ValueError):
            sd.load_rules('adapter = []\n')

    def test_the_limit_must_be_a_non_negative_integer(self):
        for value in ("-1", "true", '"0"', "1.5"):
            with self.assertRaises(ValueError, msg=value):
                sd.load_rules(CONFIG.replace("max_engine_game_files = 0", f"max_engine_game_files = {value}"))

    def test_a_class_must_be_a_list_of_strings(self):
        with self.assertRaises(ValueError):
            sd.load_rules(CONFIG.replace('adapter = ["engine/adapter_*/**"]', 'adapter = "engine/adapter_*/**"'))

    def test_an_unknown_key_is_refused(self):
        with self.assertRaises(ValueError):
            sd.load_rules(CONFIG + 'adaptor = ["x"]\n')

    def test_a_missing_class_means_an_empty_class(self):
        text = "max_engine_game_files = 0\n"
        self.assertEqual("engine_game", sd.classify("engine/adapter_a/src/lib.rs", rules(text)))


def git(repo, *args):
    subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", *args], cwd=repo, check=True, capture_output=True)


class GitRange(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: __import__("shutil").rmtree(self.dir, ignore_errors=True))
        git(self.dir, "init", "-q")
        self.write("engine/base/src/lib.rs", "a")
        git(self.dir, "add", "-A")
        git(self.dir, "commit", "-q", "-m", "one")

    def write(self, relative, text):
        path = self.dir / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def test_the_files_of_a_range_are_the_files_changed_between_its_ends(self):
        self.write("engine/adapter_x/src/lib.rs", "b")
        self.write("engine/base/src/lib.rs", "changed")
        git(self.dir, "add", "-A")
        git(self.dir, "commit", "-q", "-m", "two")
        self.assertEqual(["engine/adapter_x/src/lib.rs", "engine/base/src/lib.rs"], sd.changed_files(self.dir, "HEAD~1", "HEAD"))

    def test_a_rename_counts_both_paths(self):
        git(self.dir, "mv", "engine/base/src/lib.rs", "engine/base/src/other.rs")
        git(self.dir, "commit", "-q", "-m", "mv")
        self.assertEqual(["engine/base/src/lib.rs", "engine/base/src/other.rs"], sd.changed_files(self.dir, "HEAD~1", "HEAD"))

    def test_paths_with_spaces_and_unicode_come_back_unquoted(self):
        self.write("engine/adapter_x/src/due parole è.rs", "b")
        git(self.dir, "add", "-A")
        git(self.dir, "commit", "-q", "-m", "two")
        self.assertEqual(["engine/adapter_x/src/due parole è.rs"], sd.changed_files(self.dir, "HEAD~1", "HEAD"))

    def test_a_revision_that_looks_like_an_option_is_refused_and_does_nothing(self):
        target = self.dir / "injected.txt"
        for base in ("--output=injected.txt", "--stat", "-p"):
            with self.assertRaises(sd.SwapDiffError, msg=base):
                sd.changed_files(self.dir, base, "HEAD")
        self.assertFalse(target.exists())

    def test_the_revisions_are_resolved_to_commits(self):
        self.assertRegex(sd.resolve(self.dir, "HEAD"), r"^[0-9a-f]{40}$")
        with self.assertRaises(sd.SwapDiffError):
            sd.resolve(self.dir, "HEAD:engine/base/src/lib.rs")

    def test_a_bad_revision_is_an_error_not_an_empty_pass(self):
        with self.assertRaises(sd.SwapDiffError):
            sd.changed_files(self.dir, "nope", "HEAD")

    def test_main_prints_a_verdict_and_returns_the_exit_code(self):
        self.write("engine/base/src/lib.rs", "changed")
        git(self.dir, "add", "-A")
        git(self.dir, "commit", "-q", "-m", "two")
        config = self.dir / "swap.toml"
        config.write_text(CONFIG, encoding="utf-8")
        lines = []
        code = sd.main(["--config", str(config), "--repo", str(self.dir), "--base", "HEAD~1", "--head", "HEAD"], lines.append)
        self.assertEqual(1, code)
        self.assertTrue(any(line.startswith("FAIL") and "engine/base/src/lib.rs" in line for line in lines))
        self.assertTrue(any(line.startswith("range: ") and "..." in line for line in lines))

    def test_main_says_when_the_range_proves_nothing(self):
        config = self.dir / "swap.toml"
        config.write_text(CONFIG, encoding="utf-8")
        lines = []
        code = sd.main(["--config", str(config), "--repo", str(self.dir), "--base", "HEAD", "--head", "HEAD"], lines.append)
        self.assertEqual(1, code)
        self.assertTrue(any("no adapter or composition-root file" in line for line in lines))

    def test_main_prints_the_port_revision_line(self):
        self.write("engine/adapter_x/src/lib.rs", "b")
        self.write("engine/platform/src/contract.rs", "c")
        git(self.dir, "add", "-A")
        git(self.dir, "commit", "-q", "-m", "two")
        config = self.dir / "swap.toml"
        config.write_text(CONFIG, encoding="utf-8")
        lines = []
        self.assertEqual(0, sd.main(["--config", str(config), "--repo", str(self.dir), "--base", "HEAD~1", "--head", "HEAD"], lines.append))
        self.assertTrue(any("contains a port revision" in line for line in lines))


class RealRepository(unittest.TestCase):
    def real(self):
        root = Path(__file__).resolve().parents[2]
        return sd.load_rules((root / "gates" / "swap-diff.toml").read_text(encoding="utf-8"))

    def test_the_real_config_loads_and_sorts_known_paths(self):
        rules = self.real()
        expected = {
            "engine/adapter_window_glfw/src/lib.rs": "adapter", "engine/app_runtime/src/main.rs": "composition_root",
            "engine/platform/src/window.rs": "ports", "engine/platform/src/contract.rs": "ports",
            "engine/platform/src/headless.rs": "engine_game", "engine/platform/src/std_files.rs": "engine_game",
            "engine/platform/tests/window_no_allocation.rs": "engine_game", "engine/base/src/simd/neon.rs": "engine_game",
            "engine/frame/src/lib.rs": "engine_game", "Cargo.lock": "plumbing", "wiki/README.md": "non_code",
            "README.md": "non_code", "engine/x/README.md": "engine_game", "gates/swap-diff.toml": "non_code",
        }
        self.assertEqual(expected, {path: sd.classify(path, rules) for path in expected})


if __name__ == "__main__":
    unittest.main()
