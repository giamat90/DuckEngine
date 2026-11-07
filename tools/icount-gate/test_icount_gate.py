"""Tests of the physics instruction-count gate (card 4-23). The comparator, the thresholds and the parsing are
judged by this tool from a checkout of `main`; the benchmark binaries come from the trees under test. The bands
of the verdict are those of `gates/gates.toml [benchmark]`; the tests pin both sides of every boundary."""

import io
import json
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from unittest import mock
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import icount_gate as ig  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
GATES = """
[benchmark]
warn_percent = 5
fail_percent = 10
"""
BENCH = """
[physics_icount]
package = "tools_physics_bench"
manifest = "engine/tools_physics_bench/Cargo.toml"
rows = 20
steps = 30
runs = 2
noise_max_percent = 0.5
"""
LOG = """==4242== Callgrind, a call-graph generating cache profiler
==4242== Command: target/release/tools_physics_bench icount-workload --rows 20 --steps 30
==4242==
==4242== Events    : Ir
==4242== Collected : 162566311
==4242==
==4242== I   refs:      162,566,311
"""
WORK = "pairs 17700 manifolds 17700 hash 432dcc4c6b52004c\n"


def config(gates=GATES, bench=BENCH):
    with tempfile.TemporaryDirectory() as tmp:
        g, b = Path(tmp) / "gates.toml", Path(tmp) / "benchmarks.toml"
        g.write_text(gates, encoding="utf-8")
        b.write_text(bench, encoding="utf-8")
        return ig.read_config(g, b)


def result(count, counts=None, work=WORK):
    return {"count": count, "counts": counts or [count, count], "workload": work}


class ParseCallgrind(unittest.TestCase):
    def test_reads_the_collected_line_with_the_pid_prefix(self):
        self.assertEqual(ig.parse_callgrind(LOG), 162566311)

    def test_two_collected_lines_are_an_error_because_one_may_be_forged(self):
        # The measured program can append to the log file it finds on its own command line; valgrind prints one line.
        with self.assertRaises(ig.GateError):
            ig.parse_callgrind("==1== Collected : 5\n==1== Collected : 9\n")

    def test_a_log_without_a_count_is_an_error(self):
        for text in ("", "==1== no count here\n", "==1== Collected : abc\n"):
            with self.subTest(text=text), self.assertRaises(ig.GateError):
                ig.parse_callgrind(text)

    def test_a_zero_count_is_an_error(self):
        with self.assertRaises(ig.GateError):
            ig.parse_callgrind("==1== Collected : 0\n")

    def test_a_line_the_workload_printed_without_the_pid_prefix_is_not_a_count(self):
        # The log file holds valgrind's own messages only, but a prefix-less line must never count either.
        with self.assertRaises(ig.GateError):
            ig.parse_callgrind("Collected : 12345\n")

    def test_crlf_text_parses(self):
        self.assertEqual(ig.parse_callgrind(LOG.replace("\n", "\r\n")), 162566311)


class ReadConfig(unittest.TestCase):
    def test_reads_the_bands_and_the_workload(self):
        cfg = config()
        self.assertEqual((cfg.warn, cfg.fail), (5, 10))
        self.assertEqual((cfg.package, cfg.rows, cfg.steps, cfg.runs), ("tools_physics_bench", 20, 30, 2))
        self.assertEqual(cfg.manifest, Path("engine/tools_physics_bench/Cargo.toml"))

    def test_the_real_gates_file_reads_and_keeps_the_bands(self):
        cfg = ig.read_config(REPO / "gates" / "gates.toml", REPO / "gates" / "benchmarks.toml")
        self.assertEqual((cfg.warn, cfg.fail), (5, 10))
        self.assertEqual((cfg.package, cfg.rows, cfg.steps, cfg.runs), ("tools_physics_bench", 20, 30, 2))

    def test_a_missing_section_or_key_is_an_error(self):
        cases = (("", BENCH), (GATES, ""), (GATES, BENCH.replace("steps = 30\n", "")),
                 (GATES.replace("fail_percent = 10", ""), BENCH))
        for gates, bench in cases:
            with self.subTest(gates=gates[-20:], bench=bench[-20:]), self.assertRaises(ig.GateError):
                config(gates, bench)

    def bad(self, old, new):
        with self.assertRaises(ig.GateError):
            config(GATES.replace(old, new), BENCH.replace(old, new))

    def test_a_package_name_that_is_not_a_plain_identifier_is_refused(self):
        # The name reaches a shell command line; it must never carry a metacharacter.
        for name in ("a b", "a;rm", "$(x)", "../x", ""):
            with self.subTest(name=name):
                self.bad('package = "tools_physics_bench"', f'package = "{name}"')

    def test_the_noise_limit_may_not_exceed_a_tenth_of_the_warn_band(self):
        self.bad("noise_max_percent = 0.5", "noise_max_percent = 0.6")

    def test_bands_must_be_ordered_and_positive(self):
        self.bad("fail_percent = 10", "fail_percent = 5")
        self.bad("warn_percent = 5", "warn_percent = 0")

    def test_the_workload_must_be_sane(self):
        self.bad("rows = 20", "rows = 0")
        self.bad("rows = 20", "rows = 256")
        self.bad("steps = 30", "steps = 0")
        self.bad("runs = 2", "runs = 1")

    def test_the_manifest_must_be_a_relative_path_inside_the_tree(self):
        self.bad('manifest = "engine/tools_physics_bench/Cargo.toml"', 'manifest = "/etc/passwd"')
        self.bad('manifest = "engine/tools_physics_bench/Cargo.toml"', 'manifest = "../x/Cargo.toml"')


class Classify(unittest.TestCase):
    def verdict(self, base, head):
        return ig.classify(base, head, 5, 10)

    def test_identical_counts_pass_with_zero_delta(self):
        v = self.verdict(1000, 1000)
        self.assertEqual((v.level, v.delta_text), ("pass", "+0.00%"))

    def test_the_warn_band_is_strictly_above_five_percent(self):
        self.assertEqual(self.verdict(1000, 1050).level, "pass")
        self.assertEqual(self.verdict(1000, 1051).level, "warn")

    def test_the_fail_band_is_strictly_above_ten_percent(self):
        self.assertEqual(self.verdict(1000, 1100).level, "warn")
        self.assertEqual(self.verdict(1000, 1101).level, "fail")

    def test_an_improvement_never_fails_and_is_said(self):
        v = self.verdict(1000, 500)
        self.assertEqual((v.level, v.delta_text), ("pass", "-50.00%"))
        self.assertIn("improvement", v.message)

    def test_large_counts_use_exact_integer_arithmetic(self):
        base = 162_566_311
        edge = base * 105 // 100  # 5 % exactly is a pass, one more instruction warns
        self.assertEqual(self.verdict(base * 100, base * 105).level, "pass")
        self.assertEqual(self.verdict(base * 100, base * 105 + 1).level, "warn")
        self.assertEqual(edge, 170_694_626)

    def test_a_zero_base_is_an_error(self):
        with self.assertRaises(ig.GateError):
            self.verdict(0, 5)

    def test_the_bands_are_the_arguments_not_constants(self):
        self.assertEqual(ig.classify(1000, 1030, 2, 4).level, "warn")
        self.assertEqual(ig.classify(1000, 1050, 2, 4).level, "fail")

    def test_a_record_has_no_delta(self):
        v = ig.record_only(1234)
        self.assertEqual(v.level, "record")
        self.assertIn("no baseline, recorded only", v.message)


class Spread(unittest.TestCase):
    def test_equal_counts_have_zero_spread(self):
        ig.check_spread([100, 100, 100], 0.5)

    def test_the_limit_is_inclusive(self):
        ig.check_spread([1000, 1005], 0.5)  # exactly 0.5 %

    def test_more_noise_than_the_limit_is_an_error(self):
        with self.assertRaises(ig.GateError):
            ig.check_spread([1000, 1006], 0.5)


class Measure(unittest.TestCase):
    def runner(self, counts, work=WORK, code=0, log=None):
        calls = []
        counts = list(counts)

        def run(argv, log_path):
            calls.append(argv)
            Path(log_path).write_text(log if log is not None else f"==9== Collected : {counts.pop(0)}\n", encoding="utf-8")
            return ig.Run(code, work)

        run.calls = calls
        return run

    def test_runs_the_workload_under_callgrind_with_the_trusted_arguments(self):
        run = self.runner([100, 100])
        out = ig.measure("bin/tools_physics_bench", config(), run)
        argv = run.calls[0]
        self.assertEqual(argv[0:2], ["valgrind", "--tool=callgrind"])
        self.assertIn("--callgrind-out-file=/dev/null", argv)
        self.assertTrue(any(a.startswith("--log-file=") for a in argv))
        self.assertEqual(argv[-6:], ["bin/tools_physics_bench", "icount-workload", "--rows", "20", "--steps", "30"][-6:])
        self.assertEqual(len(run.calls), 2)
        self.assertEqual((out["count"], out["counts"]), (100, [100, 100]))

    def test_the_smallest_count_is_taken_when_the_runs_differ_within_the_noise_limit(self):
        out = ig.measure("b", config(), self.runner([1002, 1000]))
        self.assertEqual(out["count"], 1000)

    def test_noise_above_the_limit_is_an_error(self):
        with self.assertRaises(ig.GateError):
            ig.measure("b", config(), self.runner([1000, 1100]))

    def test_a_failing_workload_is_an_error(self):
        with self.assertRaises(ig.GateError):
            ig.measure("b", config(), self.runner([1, 1], code=1))

    def test_a_log_without_a_count_is_an_error(self):
        with self.assertRaises(ig.GateError):
            ig.measure("b", config(), self.runner([1, 1], log="==9== nothing\n"))

    def test_a_workload_that_does_other_work_in_the_second_run_is_an_error(self):
        outputs = iter([WORK, WORK.replace("17700", "17701")])
        counts = iter([100, 100])

        def run(argv, log_path):
            Path(log_path).write_text(f"==9== Collected : {next(counts)}\n", encoding="utf-8")
            return ig.Run(0, next(outputs))

        with self.assertRaises(ig.GateError):
            ig.measure("b", config(), run)

    def test_an_empty_workload_output_is_an_error(self):
        with self.assertRaises(ig.GateError):
            ig.measure("b", config(), self.runner([1, 1], work="  \n"))

    def test_the_workload_text_is_kept_but_bounded(self):
        out = ig.measure("b", config(), self.runner([5, 5], work="x" * 10_000))
        self.assertLessEqual(len(out["workload"]), ig.WORKLOAD_KEEP)


class Presence(unittest.TestCase):
    def trees(self, base, head):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        out = []
        for name, has in (("base", base), ("head", head)):
            root = Path(tmp.name) / name
            root.mkdir()
            if has:
                (root / "engine" / "tools_physics_bench").mkdir(parents=True)
                (root / "engine/tools_physics_bench/Cargo.toml").write_text("[package]\n", encoding="utf-8")
            out.append(root)
        return out

    def state(self, base, head):
        b, h = self.trees(base, head)
        return ig.presence(b, h, Path("engine/tools_physics_bench/Cargo.toml"))[0]

    def test_both_trees_with_the_tool_compare(self):
        self.assertEqual(self.state(True, True), "compare")

    def test_a_head_only_tool_is_recorded_without_a_baseline(self):
        self.assertEqual(self.state(False, True), "record")

    def test_neither_tree_with_the_tool_is_a_stated_skip(self):
        b, h = self.trees(False, False)
        state, message = ig.presence(b, h, Path("engine/tools_physics_bench/Cargo.toml"))
        self.assertEqual(state, "skip")
        self.assertTrue(message.startswith("not run: physics benchmark tool absent"))

    def test_a_tool_deleted_by_the_pushed_commit_is_red_never_a_skip(self):
        self.assertEqual(self.state(True, False), "fail")


class Summary(unittest.TestCase):
    def test_the_table_has_both_counts_the_delta_and_the_bands(self):
        text = ig.render_summary(ig.classify(1000, 1070, 5, 10), result(1000), result(1070), config())
        for needle in ("1000", "1070", "+7.00%", "warn", "5%", "10%", "tools_physics_bench", "--rows 20 --steps 30"):
            self.assertIn(needle, text)

    def test_a_changed_workload_output_is_mentioned(self):
        other = result(1070, work=WORK.replace("17700", "17800"))
        text = ig.render_summary(ig.classify(1000, 1070, 5, 10), result(1000), other, config())
        self.assertIn("workload output differs", text)

    def test_removed_work_is_flagged_when_the_count_drops_with_a_different_workload_output(self):
        other = result(800, work=WORK.replace("17700", "10"))
        text = ig.render_summary(ig.classify(1000, 800, 5, 10), result(1000), other, config())
        self.assertIn("work was removed?", text)
        same = ig.render_summary(ig.classify(1000, 800, 5, 10), result(1000), result(800), config())
        self.assertNotIn("work was removed?", same)

    def test_the_record_summary_says_there_is_no_baseline(self):
        text = ig.render_summary(ig.record_only(1070), None, result(1070), config())
        self.assertIn("no baseline, recorded only", text)
        self.assertIn("1070", text)


class Cli(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)
        self.gates = self.dir / "gates.toml"
        self.gates.write_text(GATES, encoding="utf-8")
        self.bench = self.dir / "benchmarks.toml"
        self.bench.write_text(BENCH, encoding="utf-8")

    def run_cli(self, *args):
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            code = ig.main([str(a) for a in args])
        return code, out.getvalue(), err.getvalue()

    def write(self, name, data):
        path = self.dir / name
        path.write_text(json.dumps(data), encoding="utf-8")
        return path

    def compare(self, base, head):
        args = ["compare", "--gates", self.gates, "--benchmarks", self.bench, "--head", self.write("head.json", result(head)),
                "--summary", self.dir / "summary.md"]
        if base is not None:
            args += ["--base", self.write("base.json", result(base))]
        return self.run_cli(*args)

    def test_equal_counts_exit_zero_and_write_the_summary(self):
        code, _, _ = self.compare(1000, 1000)
        self.assertEqual(code, 0)
        self.assertIn("+0.00%", (self.dir / "summary.md").read_text(encoding="utf-8"))

    def test_a_warn_exits_zero_with_a_warning_annotation(self):
        code, out, _ = self.compare(1000, 1080)
        self.assertEqual(code, 0)
        self.assertIn("::warning::", out)

    def test_a_fail_exits_one_with_an_error_annotation(self):
        code, out, err = self.compare(1000, 1200)
        self.assertEqual(code, 1)
        self.assertIn("::error::", out + err)

    def test_no_baseline_exits_zero_and_says_so(self):
        code, out, _ = self.compare(None, 1000)
        self.assertEqual(code, 0)
        self.assertIn("no baseline, recorded only", (self.dir / "summary.md").read_text(encoding="utf-8"))

    def test_the_bands_come_from_the_gates_file_given(self):
        self.gates.write_text(GATES.replace("warn_percent = 5", "warn_percent = 1").replace("fail_percent = 10", "fail_percent = 2"),
                              encoding="utf-8")
        self.assertEqual(self.compare(1000, 1030)[0], 1)

    def test_a_broken_result_file_is_red(self):
        (self.dir / "head.json").write_text("{not json", encoding="utf-8")
        code, _, err = self.run_cli("compare", "--gates", self.gates, "--benchmarks", self.bench, "--head", self.dir / "head.json",
                                    "--summary", self.dir / "s.md")
        self.assertEqual(code, 1)
        self.assertTrue(err)

    def test_a_result_without_a_count_is_red(self):
        path = self.write("head.json", {"counts": [1]})
        code, _, _ = self.run_cli("compare", "--gates", self.gates, "--benchmarks", self.bench, "--head", path, "--summary", self.dir / "s.md")
        self.assertEqual(code, 1)

    def test_presence_skip_writes_the_stated_line_and_the_outputs(self):
        base, head = self.dir / "base", self.dir / "head"
        base.mkdir()
        head.mkdir()
        out_file, summary = self.dir / "out.txt", self.dir / "sum.md"
        code, out, _ = self.run_cli("presence", "--gates", self.gates, "--benchmarks", self.bench, "--base", base, "--head", head,
                                    "--github-output", out_file, "--summary", summary)
        self.assertEqual(code, 0)
        self.assertIn("not run: physics benchmark tool absent", summary.read_text(encoding="utf-8"))
        text = out_file.read_text(encoding="utf-8")
        self.assertIn("head=false", text)
        self.assertIn("base=false", text)

    def test_presence_with_a_deleted_tool_is_red(self):
        base, head = self.dir / "base", self.dir / "head"
        (base / "engine/tools_physics_bench").mkdir(parents=True)
        (base / "engine/tools_physics_bench/Cargo.toml").write_text("x", encoding="utf-8")
        head.mkdir()
        code, _, err = self.run_cli("presence", "--gates", self.gates, "--benchmarks", self.bench, "--base", base, "--head", head)
        self.assertEqual(code, 1)
        self.assertIn("deleted", err)

    def test_presence_record_and_compare_set_the_outputs(self):
        for base_has, expect_base in ((False, "base=false"), (True, "base=true")):
            base, head = self.dir / f"b{base_has}", self.dir / f"h{base_has}"
            for tree, has in ((base, base_has), (head, True)):
                tree.mkdir()
                if has:
                    (tree / "engine/tools_physics_bench").mkdir(parents=True)
                    (tree / "engine/tools_physics_bench/Cargo.toml").write_text("x", encoding="utf-8")
            out_file = self.dir / f"o{base_has}.txt"
            code, _, _ = self.run_cli("presence", "--gates", self.gates, "--benchmarks", self.bench, "--base", base, "--head", head,
                                      "--github-output", out_file)
            self.assertEqual(code, 0)
            text = out_file.read_text(encoding="utf-8")
            self.assertIn("head=true", text)
            self.assertIn(expect_base, text)
            self.assertIn("package=tools_physics_bench", text)

    def test_config_failure_is_red(self):
        self.gates.write_text("[benchmark]\n", encoding="utf-8")
        code, _, err = self.run_cli("presence", "--gates", self.gates, "--benchmarks", self.bench, "--base", self.dir, "--head", self.dir)
        self.assertEqual(code, 1)
        self.assertTrue(err)


class MoreConfig(unittest.TestCase):
    def bad(self, old, new):
        with self.assertRaises(ig.GateError):
            config(GATES, BENCH.replace(old, new))

    def test_a_noise_limit_of_zero_and_of_exactly_a_tenth_of_the_band_are_allowed(self):
        self.assertEqual(config(GATES, BENCH.replace("0.5", "0")).noise_max_percent, 0)
        self.assertEqual(config(GATES, BENCH).noise_max_percent, 0.5)

    def test_a_noise_limit_that_is_negative_not_a_number_or_not_a_multiple_of_a_tenth_is_refused(self):
        for value in ("-0.1", '"0.5"', "true", "0.35"):
            with self.subTest(value=value):
                self.bad("noise_max_percent = 0.5", f"noise_max_percent = {value}")

    def test_a_manifest_that_is_not_a_string_or_is_empty_is_refused(self):
        for value in ("5", '""', "'\\share/x'"):
            with self.subTest(value=value):
                self.bad('manifest = "engine/tools_physics_bench/Cargo.toml"', f"manifest = {value}")

    def test_a_boolean_or_text_where_a_number_is_needed_is_refused(self):
        for old, new in (("rows = 20", "rows = true"), ("rows = 20", 'rows = "20"'), ("steps = 30", "steps = 30.5")):
            with self.subTest(new=new):
                self.bad(old, new)

    def test_equal_bands_are_refused(self):
        with self.assertRaises(ig.GateError):
            config(GATES.replace("fail_percent = 10", "fail_percent = 5"), BENCH)


class MoreClassify(unittest.TestCase):
    def test_equal_counts_are_not_called_an_improvement(self):
        self.assertNotIn("improvement", ig.classify(1000, 1000, 5, 10).message)

    def test_a_rounding_of_the_delta_is_to_the_nearest_hundredth(self):
        self.assertEqual(ig.classify(3, 4, 50, 60).delta_text, "+33.33%")
        self.assertEqual(ig.classify(3, 2, 50, 60).delta_text, "-33.33%")

    def test_a_zero_head_is_an_error(self):
        with self.assertRaises(ig.GateError):
            ig.classify(5, 0, 5, 10)

    def test_the_spread_boundary_is_inclusive_for_a_limit_of_a_tenth(self):
        ig.check_spread([10000, 10010], 0.1)
        with self.assertRaises(ig.GateError):
            ig.check_spread([10000, 10011], 0.1)


class DefaultRunner(unittest.TestCase):
    def test_returns_the_exit_code_and_the_output_without_raising(self):
        done = ig.default_runner([sys.executable, "-c", "import sys; print('out'); sys.exit(3)"], "unused.log")
        self.assertEqual((done.returncode, done.stdout.strip()), (3, "out"))


class MoreCli(Cli):
    def test_measure_writes_the_result_file_and_prints_the_count(self):
        counts = iter([777, 777])

        def fake(argv, log_path):
            Path(log_path).write_text(f"==3== Collected : {next(counts)}\n", encoding="utf-8")
            return ig.Run(0, WORK)

        out_file = self.dir / "out.json"
        with mock.patch.object(ig, "default_runner", fake):
            code, out, _ = self.run_cli("measure", "--gates", self.gates, "--benchmarks", self.bench,
                                        "--bench", "bin/x", "--out", out_file)
        self.assertEqual(code, 0)
        self.assertEqual(json.loads(out_file.read_text(encoding="utf-8"))["count"], 777)
        self.assertIn("777", out)

    def test_measure_with_a_failing_workload_is_red_and_writes_nothing(self):
        out_file = self.dir / "out.json"
        with mock.patch.object(ig, "default_runner", lambda argv, log: ig.Run(2, "")):
            code, _, _ = self.run_cli("measure", "--gates", self.gates, "--benchmarks", self.bench,
                                      "--bench", "bin/x", "--out", out_file)
        self.assertEqual(code, 1)
        self.assertFalse(out_file.exists())

    def test_a_count_of_zero_negative_text_or_boolean_in_a_result_is_red(self):
        for count in (0, -5, "12", True, None):
            with self.subTest(count=count):
                path = self.write("head.json", {"count": count})
                code, _, _ = self.run_cli("compare", "--gates", self.gates, "--benchmarks", self.bench,
                                          "--head", path, "--summary", self.dir / "s.md")
                self.assertEqual(code, 1)

    def test_the_summary_has_a_base_row_only_when_there_is_a_base(self):
        self.compare(1000, 1000)
        self.assertIn("| base |", (self.dir / "summary.md").read_text(encoding="utf-8"))
        (self.dir / "summary.md").unlink()
        self.compare(None, 1000)
        self.assertNotIn("| base |", (self.dir / "summary.md").read_text(encoding="utf-8"))

    def test_presence_record_announces_itself(self):
        base, head = self.dir / "b", self.dir / "h"
        base.mkdir()
        (head / "engine/tools_physics_bench").mkdir(parents=True)
        (head / "engine/tools_physics_bench/Cargo.toml").write_text("x", encoding="utf-8")
        code, out, _ = self.run_cli("presence", "--gates", self.gates, "--benchmarks", self.bench,
                                    "--base", base, "--head", head)
        self.assertEqual(code, 0)
        self.assertIn("no baseline, recorded only", out)

    def test_every_required_argument_is_required(self):
        full = {"presence": ["--gates", "g", "--benchmarks", "b", "--base", "x", "--head", "y"],
                "measure": ["--gates", "g", "--benchmarks", "b", "--bench", "x", "--out", "y"],
                "compare": ["--gates", "g", "--benchmarks", "b", "--head", "y"]}
        for command, args in full.items():
            for i in range(0, len(args), 2):
                with self.subTest(command=command, missing=args[i]), redirect_stderr(io.StringIO()):
                    with self.assertRaises(SystemExit):
                        ig.main([command] + args[:i] + args[i + 2:])
        with redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            ig.main([])


if __name__ == "__main__":
    unittest.main()
