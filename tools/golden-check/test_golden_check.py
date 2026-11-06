"""Tests of the golden hash checker (card 4-20). The golden file is judged by the tool of `main`; a run
that is short, long, mislabelled or different must fail, and only the absence of the whole crate may skip."""

import io
import os
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import golden_check as gc  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
A = "1a6b18bb68e76076"
B = "00000000000000ff"
C = "ffffffffffffffff"


def output_lines(feature="on", fma="false", os_name="linux", arch="x86_64", scenes=None, end=True, fmt="1"):
    """A run of two small scenes: `pyramid` (7 steps, stride 5: steps 1, 5, 7) and `chain` (3 steps, stride 5: 1, 3)."""
    scenes = scenes if scenes is not None else {
        "pyramid": (7, 5, {1: A, 5: B, 7: C}),
        "chain": (3, 5, {1: C, 3: A}),
    }
    lines = [f"GOLDEN|format|{fmt}", f"GOLDEN|feature|cross-platform-determinism|{feature}",
             f"GOLDEN|platform|{os_name}|{arch}", f"GOLDEN|fma|{fma}"]
    for name, (steps, stride, hashes) in scenes.items():
        lines.append(f"GOLDEN|scene|{name}|{steps}|{stride}")
        lines += [f"GOLDEN|hash|{name}|{step}|{value}" for step, value in hashes.items()]
    if end:
        lines.append(f"GOLDEN|end|{len(lines)}")
    return lines


def text(lines, noise=True, eol="\n"):
    body = list(lines)
    if noise:  # cargo prints other things around the lines
        body = ["running 4 tests", "test print_the_golden_lines ... GOLDEN-not-a-line"] + body + ["test result: ok."]
    return eol.join(body) + eol


def parsed(**kwargs):
    return gc.parse_output(text(output_lines(**kwargs)))


def golden_of(**kwargs):
    return gc.parse_golden(gc.render_golden(parsed(**kwargs), toolchain="1.94.1", recorded="test"))


class Parse(unittest.TestCase):
    def test_a_complete_run_is_read(self):
        run = parsed()
        self.assertEqual((run.format, run.feature, run.os, run.arch, run.fma), (1, "on", "linux", "x86_64", False))
        self.assertEqual(list(run.scenes), ["pyramid", "chain"])
        self.assertEqual(run.scenes["pyramid"].hashes, {1: A, 5: B, 7: C})

    def test_crlf_line_endings_give_the_same_run(self):
        self.assertEqual(gc.parse_output(text(output_lines(), eol="\r\n")), parsed())

    def test_a_missing_end_line_is_refused(self):
        with self.assertRaisesRegex(gc.GoldenError, "end"):
            gc.parse_output(text(output_lines(end=False)))

    def test_a_wrong_end_count_is_refused(self):
        lines = output_lines()
        lines[-1] = "GOLDEN|end|3"
        with self.assertRaisesRegex(gc.GoldenError, "count"):
            gc.parse_output(text(lines))

    def test_no_golden_line_at_all_is_refused(self):
        with self.assertRaisesRegex(gc.GoldenError, "no GOLDEN line"):
            gc.parse_output("running 0 tests\n")

    def test_lines_after_the_end_are_refused(self):
        with self.assertRaisesRegex(gc.GoldenError, "after"):
            gc.parse_output(text(output_lines() + [f"GOLDEN|hash|chain|3|{A}"], noise=False))

    def test_a_hash_that_is_not_sixteen_lowercase_hex_digits_is_refused(self):
        for bad in ("1A6B18BB68E76076", "1a6b18bb68e7607", "zz6b18bb68e76076", "0x1a6b18bb68e76076"):
            with self.subTest(bad=bad):
                lines = output_lines(scenes={"s": (1, 5, {1: bad})})
                with self.assertRaisesRegex(gc.GoldenError, "hash"):
                    gc.parse_output(text(lines))

    def test_a_duplicate_scene_or_step_is_refused(self):
        lines = output_lines()
        lines.insert(-1, "GOLDEN|scene|chain|3|5")
        with self.assertRaisesRegex(gc.GoldenError, "twice"):
            gc.parse_output(text(lines))
        lines = output_lines()
        lines.insert(-1, f"GOLDEN|hash|chain|3|{A}")
        with self.assertRaisesRegex(gc.GoldenError, "twice"):
            gc.parse_output(text(lines))

    def test_a_hash_of_an_undeclared_scene_is_refused(self):
        lines = output_lines()
        lines.insert(-1, f"GOLDEN|hash|ghost|1|{A}")
        with self.assertRaisesRegex(gc.GoldenError, "ghost"):
            gc.parse_output(text(lines))

    def test_an_unknown_kind_or_bad_field_count_is_refused(self):
        for bad in ("GOLDEN|nonsense|x", "GOLDEN|hash|chain|3", "GOLDEN|scene|chain|three|5", "GOLDEN|fma|maybe"):
            with self.subTest(bad=bad):
                lines = output_lines()
                lines.insert(-1, bad)
                with self.assertRaises(gc.GoldenError):
                    gc.parse_output(text(lines))

    def test_checkpoints_must_be_the_first_step_every_stride_and_the_last(self):
        lines = output_lines(scenes={"s": (7, 5, {1: A, 5: B})})  # the last step is missing
        with self.assertRaisesRegex(gc.GoldenError, "checkpoints"):
            gc.parse_output(text(lines))


class Golden(unittest.TestCase):
    def test_render_then_parse_gives_the_same_scenes(self):
        golden = golden_of()
        self.assertEqual(golden.scenes, parsed().scenes)
        self.assertEqual(golden.format, 1)

    def test_the_rendered_file_is_lf_only_and_ends_with_one_newline(self):
        data = gc.render_golden(parsed(), toolchain="1.94.1", recorded="test")
        self.assertNotIn("\r", data)
        self.assertTrue(data.endswith("\n") and not data.endswith("\n\n"))

    def test_a_golden_without_scenes_or_with_wrong_format_or_feature_is_refused(self):
        good = gc.render_golden(parsed(), toolchain="1.94.1", recorded="test")
        for name, bad in (("format", good.replace("format = 1", "format = 2", 1)),
                          ("feature", good.replace('feature = "cross-platform-determinism"', 'feature = "none"', 1)),
                          ("scenes", good[:good.index("[[scene]]")]),
                          ("syntax", good + "\n[[scene")):
            with self.subTest(name=name), self.assertRaises(gc.GoldenError):
                gc.parse_golden(bad)

    def test_a_golden_with_a_missing_checkpoint_is_refused(self):
        good = gc.render_golden(parsed(), toolchain="1.94.1", recorded="test")
        with self.assertRaisesRegex(gc.GoldenError, "checkpoints"):
            gc.parse_golden(good.replace(f'5 = "{B}"\n', "", 1))

    def test_malformed_but_valid_toml_is_a_golden_error_not_a_crash(self):
        head = 'format = 1\nfeature = "cross-platform-determinism"\n'
        scene = '[[scene]]\nname = "a"\nsteps = {}\nstride = 1\n[scene.hash]\n1 = "' + A + '"\n'
        for body in ("scene = 3\n", '[[scene]]\nname = ["a"]\nsteps = 1\nstride = 1\nhash = 3\n',
                     scene.format('"1"'), scene.format("true")):
            with self.subTest(body=body), self.assertRaises(gc.GoldenError):
                gc.parse_golden(head + body)

    def test_a_huge_step_count_is_refused_before_it_is_expanded(self):
        lines = output_lines(scenes={"s": (10**8, 1, {1: A})})
        with self.assertRaisesRegex(gc.GoldenError, "between"):
            gc.parse_output(text(lines))

    def test_a_step_with_a_leading_zero_is_refused(self):
        lines = [line.replace("|pyramid|5|", "|pyramid|05|") for line in output_lines()]
        with self.assertRaises(gc.GoldenError):
            gc.parse_output(text(lines))

    def test_the_stored_golden_file_is_well_formed(self):
        path = REPO / "gates" / "golden" / "physics-replay.toml"
        golden = gc.parse_golden(path.read_text(encoding="utf-8"))
        self.assertGreaterEqual(len(golden.scenes), 12)
        self.assertNotIn(b"\r", path.read_bytes())


class Compare(unittest.TestCase):
    def test_equal_runs_have_no_problem(self):
        self.assertEqual(gc.compare(golden_of(), parsed()), [])

    def test_a_different_hash_names_the_scene_and_the_first_step(self):
        other = parsed(scenes={"pyramid": (7, 5, {1: A, 5: C, 7: B}), "chain": (3, 5, {1: C, 3: A})})
        problems = gc.compare(golden_of(), other)
        self.assertEqual(len(problems), 1)
        self.assertIn("pyramid", problems[0])
        self.assertIn("step 5", problems[0])
        self.assertIn(B, problems[0])
        self.assertIn(C, problems[0])

    def test_a_missing_scene_is_a_problem(self):
        other = parsed(scenes={"pyramid": (7, 5, {1: A, 5: B, 7: C})})
        self.assertTrue(any("chain" in p and "missing" in p for p in gc.compare(golden_of(), other)))

    def test_an_extra_scene_is_a_problem(self):
        golden = golden_of(scenes={"pyramid": (7, 5, {1: A, 5: B, 7: C})})
        self.assertTrue(any("chain" in p and "not in the golden" in p for p in gc.compare(golden, parsed())))

    def test_changed_steps_or_stride_are_problems(self):
        other = parsed(scenes={"pyramid": (7, 5, {1: A, 5: B, 7: C}), "chain": (4, 2, {1: C, 2: A, 4: A})})
        problems = gc.compare(golden_of(), other)
        self.assertTrue(any("chain" in p and "steps" in p for p in problems))

    def test_every_differing_scene_is_listed_not_only_the_first(self):
        other = parsed(scenes={"pyramid": (7, 5, {1: B, 5: B, 7: C}), "chain": (3, 5, {1: C, 3: B})})
        self.assertEqual(len(gc.compare(golden_of(), other)), 2)


class Environment(unittest.TestCase):
    def test_a_good_run_has_no_environment_problem(self):
        for os_name in ("linux", "windows"):
            with self.subTest(os=os_name):
                self.assertEqual(gc.environment_problems(parsed(os_name=os_name)), [])

    def test_feature_off_is_refused(self):
        self.assertTrue(any("feature" in p for p in gc.environment_problems(parsed(feature="off"))))

    def test_fma_on_is_refused(self):
        self.assertTrue(any("fma" in p for p in gc.environment_problems(parsed(fma="true"))))

    def test_a_platform_that_is_not_gated_is_refused(self):
        for os_name, arch in (("linux", "aarch64"), ("macos", "x86_64"), ("windows", "aarch64")):
            with self.subTest(os=os_name, arch=arch):
                self.assertTrue(any("platform" in p for p in gc.environment_problems(parsed(os_name=os_name, arch=arch))))

    def test_another_format_is_refused(self):
        with self.assertRaisesRegex(gc.GoldenError, "format"):
            parsed(fmt="2")


class Presence(unittest.TestCase):
    def make(self, trusted_crate, head_crate, head_recorder=True):
        root = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: __import__("shutil").rmtree(root, ignore_errors=True))
        for name, crate in (("trusted", trusted_crate), ("head", head_crate)):
            (root / name / "engine" / "physics").mkdir(parents=True)
            if crate:
                (root / name / "engine" / "physics" / "Cargo.toml").write_text("[package]\n")
        if head_crate and head_recorder:
            recorder = root / "head" / "engine" / "physics" / "tests" / "determinism"
            recorder.mkdir(parents=True)
            (recorder / "golden.rs").write_text("// recorder" + chr(10))
        return root / "trusted", root / "head"

    def test_a_crate_in_the_head_with_its_recorder_runs_the_job(self):
        self.assertEqual(gc.presence(*self.make(False, True))[0], "run")
        self.assertEqual(gc.presence(*self.make(True, True))[0], "run")

    def test_no_crate_anywhere_is_the_only_skip(self):
        state, message = gc.presence(*self.make(False, False))
        self.assertEqual(state, "skip")
        self.assertIn("not run: physics crate absent", message)

    def test_a_crate_on_main_that_the_head_lacks_is_a_failure_not_a_skip(self):
        state, message = gc.presence(*self.make(True, False))
        self.assertEqual(state, "fail")
        self.assertIn("deleted", message)

    def test_a_crate_without_its_recorder_is_a_failure_not_a_skip(self):
        state, message = gc.presence(*self.make(False, True, head_recorder=False))
        self.assertEqual(state, "fail")
        self.assertIn("tests/determinism/golden.rs", message)

    def test_the_recorder_path_is_the_module_of_the_determinism_target(self):
        # Pinned: the recorder is `golden` in tests/determinism (card 4-20a). A move needs this line and the job changed.
        self.assertEqual(gc.RECORDER.as_posix(), "engine/physics/tests/determinism/golden.rs")

    def test_a_folder_without_a_manifest_does_not_count_as_a_crate(self):
        # main has `engine/physics/clippy.toml` (card 4-19) and no crate: that is the skip, not a run.
        trusted, head = self.make(False, False)
        (head / "engine" / "physics" / "clippy.toml").write_text("x = 1\n")
        self.assertEqual(gc.presence(trusted, head)[0], "skip")


class Flags(unittest.TestCase):
    def test_clean_flags_pass(self):
        self.assertEqual(gc.flag_problems({"RUSTFLAGS": "-D warnings"}, []), [])
        self.assertEqual(gc.flag_problems({}, ["[build]\njobs = 2\n"]), [])

    def test_native_cpu_and_fma_are_refused_in_any_variable(self):
        for name in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS", "RUSTDOCFLAGS"):
            for value in ("-C target-cpu=native", "-Ctarget-cpu=native", "-C target-feature=+fma", "-Ctarget-feature=+avx2,+fma",
                          "-C\x1ftarget-cpu=native"):
                with self.subTest(name=name, value=value):
                    self.assertTrue(gc.flag_problems({name: value}, []))

    def test_a_cargo_config_with_such_flags_is_refused(self):
        for config in ('[build]\nrustflags = ["-C", "target-cpu=native"]\n', "[target.x86_64-pc-windows-msvc]\nrustflags=['-Ctarget-feature=+fma']\n"):
            with self.subTest(config=config):
                self.assertTrue(gc.flag_problems({}, [config]))

    def test_other_target_cpu_values_are_refused_too(self):
        self.assertTrue(gc.flag_problems({"RUSTFLAGS": "-C target-cpu=haswell"}, []))

    def test_the_per_target_rustflags_variables_are_scanned(self):
        env = {"CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS": "-C target-cpu=native"}
        self.assertTrue(gc.flag_problems(env, []))

    def test_target_feature_minus_fma_is_allowed(self):
        self.assertEqual(gc.flag_problems({"RUSTFLAGS": "-C target-feature=-fma"}, []), [])


class Cli(unittest.TestCase):
    def run_cli(self, *argv):
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            code = gc.main(list(argv))
        return code, out.getvalue(), err.getvalue()

    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: __import__("shutil").rmtree(self.dir, ignore_errors=True))
        self.golden = self.dir / "golden.toml"
        self.golden.write_text(gc.render_golden(parsed(), toolchain="1.94.1", recorded="test"), encoding="utf-8", newline="\n")
        self.output = self.dir / "out.txt"
        self.output.write_text(text(output_lines()), encoding="utf-8")

    def test_verify_passes_on_the_same_run_and_writes_a_table(self):
        summary = self.dir / "summary.md"
        code, out, _ = self.run_cli("verify", "--golden", str(self.golden), "--output", str(self.output), "--summary", str(summary))
        self.assertEqual(code, 0, out)
        self.assertIn("pyramid", summary.read_text(encoding="utf-8"))

    def test_verify_fails_on_a_missing_golden(self):
        code, _, err = self.run_cli("verify", "--golden", str(self.dir / "none.toml"), "--output", str(self.output))
        self.assertEqual(code, 1)
        self.assertIn("golden", err)

    def test_verify_fails_on_a_missing_output_file(self):
        code, _, _ = self.run_cli("verify", "--golden", str(self.golden), "--output", str(self.dir / "none.txt"))
        self.assertEqual(code, 1)

    def test_verify_fails_on_a_different_hash_and_says_where(self):
        self.output.write_text(text(output_lines(scenes={"pyramid": (7, 5, {1: A, 5: C, 7: C}), "chain": (3, 5, {1: C, 3: A})})))
        code, _, err = self.run_cli("verify", "--golden", str(self.golden), "--output", str(self.output))
        self.assertEqual(code, 1)
        self.assertIn("pyramid", err)
        self.assertIn("step 5", err)

    def test_verify_fails_for_the_wrong_feature_flag_in_the_header(self):
        self.output.write_text(text(output_lines(feature="off")))
        self.assertEqual(self.run_cli("verify", "--golden", str(self.golden), "--output", str(self.output))[0], 1)

    def test_verify_fails_on_a_bad_golden_file(self):
        self.golden.write_text("format = 1\n")
        self.assertEqual(self.run_cli("verify", "--golden", str(self.golden), "--output", str(self.output))[0], 1)

    def test_lint_checks_the_golden_alone(self):
        self.assertEqual(self.run_cli("lint", "--golden", str(self.golden))[0], 0)
        self.golden.write_text("not toml [")
        self.assertEqual(self.run_cli("lint", "--golden", str(self.golden))[0], 1)

    def test_record_refuses_without_the_explicit_flag(self):
        target = self.dir / "new.toml"
        code, _, err = self.run_cli("record", "--output", str(self.output), "--golden", str(target))
        self.assertEqual(code, 1)
        self.assertIn("governance", err)
        self.assertFalse(target.exists())

    def test_record_with_the_flag_writes_a_file_that_verifies(self):
        target = self.dir / "new.toml"
        code, _, _ = self.run_cli("record", "--output", str(self.output), "--golden", str(target), "--toolchain", "1.94.1",
                                  "--recorded", "test", "--i-am-changing-the-golden-file")
        self.assertEqual(code, 0)
        self.assertEqual(self.run_cli("verify", "--golden", str(target), "--output", str(self.output))[0], 0)
        self.assertNotIn(b"\r", target.read_bytes())

    def test_record_refuses_a_run_with_the_feature_off(self):
        self.output.write_text(text(output_lines(feature="off")))
        target = self.dir / "new.toml"
        code, _, _ = self.run_cli("record", "--output", str(self.output), "--golden", str(target), "--i-am-changing-the-golden-file")
        self.assertEqual(code, 1)
        self.assertFalse(target.exists())

    def test_presence_writes_the_github_output(self):
        root = self.dir / "r"
        (root / "trusted" / "engine" / "physics").mkdir(parents=True)
        (root / "head" / "engine" / "physics").mkdir(parents=True)
        gh = self.dir / "gh_output.txt"
        summary = self.dir / "summary.md"
        code, out, _ = self.run_cli("presence", "--trusted", str(root / "trusted"), "--head", str(root / "head"),
                                    "--github-output", str(gh), "--summary", str(summary))
        self.assertEqual(code, 0)
        self.assertIn("present=false", gh.read_text())
        self.assertIn("::notice::not run: physics crate absent", out)
        self.assertIn("not run", summary.read_text())

    def test_presence_fails_when_the_crate_vanished(self):
        root = self.dir / "r"
        (root / "trusted" / "engine" / "physics").mkdir(parents=True)
        (root / "trusted" / "engine" / "physics" / "Cargo.toml").write_text("x")
        (root / "head" / "engine" / "physics").mkdir(parents=True)
        code, _, err = self.run_cli("presence", "--trusted", str(root / "trusted"), "--head", str(root / "head"))
        self.assertEqual(code, 1)
        self.assertIn("deleted", err)

    def test_flags_reads_the_environment_and_the_repo_config(self):
        repo = self.dir / "repo"
        (repo / ".cargo").mkdir(parents=True)
        (repo / ".cargo" / "config.toml").write_text('[build]\nrustflags = ["-C", "target-cpu=native"]\n')
        old = dict(os.environ)
        self.addCleanup(lambda: (os.environ.clear(), os.environ.update(old)))
        os.environ.pop("RUSTFLAGS", None)
        self.assertEqual(self.run_cli("flags", "--repo", str(repo))[0], 1)
        (repo / ".cargo" / "config.toml").unlink()
        self.assertEqual(self.run_cli("flags", "--repo", str(repo))[0], 0)
        os.environ["RUSTFLAGS"] = "-C target-cpu=native"
        self.assertEqual(self.run_cli("flags", "--repo", str(repo))[0], 1)


if __name__ == "__main__":
    unittest.main()
