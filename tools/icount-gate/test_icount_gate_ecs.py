"""Tests of the ECS profile of the instruction-count gate (card 7-6). The tool is the one of the physics gate
(card 4-23) and the audio profile (card 5-15); a profile names the benchmark table of `gates/benchmarks.toml`, the
workload flags of the benchmark binary and the words of the summary. The ECS profile counts the instructions of the
serial query workload of `tools_ecs_bench icount-workload` (one, two and three components over one world), base tree
against pushed commit on the same runner, so the verdict does not depend on the CPU type of the runner."""

import io
import json
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
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

[ecs_icount]
package = "tools_ecs_bench"
manifest = "engine/tools_ecs_bench/Cargo.toml"
entities = 8192
frames = 8
runs = 2
noise_max_percent = 0.5
"""
WORK = "icount-workload entities=8192 frames=8 checksum=0901857b5d466f80\n"


def ecs_config(bench=BENCH):
    with tempfile.TemporaryDirectory() as tmp:
        g, b = Path(tmp) / "gates.toml", Path(tmp) / "benchmarks.toml"
        g.write_text(GATES, encoding="utf-8")
        b.write_text(bench, encoding="utf-8")
        return ig.read_config(g, b, ig.ECS)


class ReadEcsConfig(unittest.TestCase):
    def test_reads_the_bands_and_the_entity_workload(self):
        cfg = ecs_config()
        self.assertEqual((cfg.warn, cfg.fail), (5, 10))
        self.assertEqual((cfg.package, cfg.runs), ("tools_ecs_bench", 2))
        self.assertEqual(cfg.manifest, Path("engine/tools_ecs_bench/Cargo.toml"))
        self.assertEqual(cfg.workload, (("--entities", 8192), ("--frames", 8)))
        self.assertEqual(cfg.title, "ECS")

    def test_the_real_gates_file_reads_the_ecs_profile(self):
        cfg = ig.read_config(REPO / "gates" / "gates.toml", REPO / "gates" / "benchmarks.toml", ig.ECS)
        self.assertEqual((cfg.warn, cfg.fail), (5, 10))
        self.assertEqual((cfg.package, cfg.runs), ("tools_ecs_bench", 2))
        self.assertEqual(dict(cfg.workload)["--entities"], 8192)

    def test_the_other_profiles_are_unchanged(self):
        cfg = ig.read_config(REPO / "gates" / "gates.toml", REPO / "gates" / "benchmarks.toml")
        self.assertEqual((cfg.package, cfg.rows, cfg.steps), ("tools_physics_bench", 20, 30))
        self.assertEqual(sorted(ig.PROFILES), ["audio", "ecs", "physics", "physics3d"])

    def test_a_missing_ecs_table_is_an_error_not_a_fallback_to_physics(self):
        with self.assertRaises(ig.GateError):
            ecs_config(BENCH.split("[ecs_icount]")[0])

    def test_the_workload_must_be_sane(self):
        for old, new in (("entities = 8192", "entities = 0"), ("entities = 8192", "entities = 1000001"),
                         ("frames = 8", "frames = 0"), ("frames = 8", "frames = 1000001"),
                         ("runs = 2", "runs = 1"), ('package = "tools_ecs_bench"', 'package = "a;b"')):
            with self.subTest(old=old, new=new), self.assertRaises(ig.GateError):
                ecs_config(BENCH.replace(old, new))

    def test_the_noise_limit_may_not_exceed_a_tenth_of_the_warn_band(self):
        ecs = BENCH.split("[ecs_icount]")[1].replace("noise_max_percent = 0.5", "noise_max_percent = 0.6")
        with self.assertRaises(ig.GateError):
            ecs_config(BENCH.split("[ecs_icount]")[0] + "[ecs_icount]" + ecs)


class MeasureEcs(unittest.TestCase):
    def test_runs_the_workload_with_the_trusted_arguments(self):
        calls = []

        def run(argv, log_path):
            calls.append(argv)
            Path(log_path).write_text("==9== Collected : 5000\n", encoding="utf-8")
            return ig.Run(0, WORK)

        out = ig.measure("bin/tools_ecs_bench", ecs_config(), run)
        self.assertEqual(calls[0][-5:], ["icount-workload", "--entities", "8192", "--frames", "8"])
        self.assertEqual(out["count"], 5000)
        self.assertEqual(out["workload"], WORK.strip())


class EcsWords(unittest.TestCase):
    def test_the_skip_line_names_the_ecs_benchmark(self):
        with tempfile.TemporaryDirectory() as tmp:
            base, head = Path(tmp) / "base", Path(tmp) / "head"
            base.mkdir()
            head.mkdir()
            state, message = ig.presence(base, head, Path("engine/tools_ecs_bench/Cargo.toml"), ig.ECS.skip)
        self.assertEqual(state, "skip")
        self.assertTrue(message.startswith("not run: ECS benchmark tool absent"), message)

    def test_the_summary_names_the_ecs_workload_and_not_physics(self):
        cfg = ecs_config()
        head = {"count": 1070, "counts": [1070, 1070], "workload": WORK}
        base = {"count": 1000, "counts": [1000, 1000], "workload": WORK}
        text = ig.render_summary(ig.classify(1000, 1070, 5, 10), base, head, cfg)
        self.assertIn("ECS instruction count (tools_ecs_bench icount-workload --entities 8192 --frames 8", text)
        self.assertNotIn("Physics", text)


class EcsCli(unittest.TestCase):
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

    def trees(self, with_tool):
        base, head = self.dir / "base", self.dir / "head"
        for tree in (base, head):
            tree.mkdir()
            if with_tool:
                (tree / "engine" / "tools_ecs_bench").mkdir(parents=True)
                (tree / "engine/tools_ecs_bench/Cargo.toml").write_text("[package]\n", encoding="utf-8")
        return base, head

    def test_presence_skips_with_the_ecs_line_and_sets_the_ecs_package(self):
        base, head = self.trees(False)
        out_file, summary = self.dir / "out.txt", self.dir / "sum.md"
        code, _, _ = self.run_cli("presence", "--benchmark", "ecs", "--gates", self.gates, "--benchmarks", self.bench,
                                  "--base", base, "--head", head, "--github-output", out_file, "--summary", summary)
        self.assertEqual(code, 0)
        self.assertIn("ECS instruction count: not run: ECS benchmark tool absent", summary.read_text(encoding="utf-8"))
        text = out_file.read_text(encoding="utf-8")
        self.assertIn("package=tools_ecs_bench", text)
        self.assertIn("head=false", text)

    def test_presence_compares_when_both_trees_have_the_tool(self):
        base, head = self.trees(True)
        out_file = self.dir / "out.txt"
        code, _, _ = self.run_cli("presence", "--benchmark", "ecs", "--gates", self.gates, "--benchmarks", self.bench,
                                  "--base", base, "--head", head, "--github-output", out_file)
        self.assertEqual(code, 0)
        text = out_file.read_text(encoding="utf-8")
        self.assertIn("head=true", text)
        self.assertIn("base=true", text)

    def test_presence_records_only_when_just_the_head_has_the_tool(self):
        base, head = self.trees(False)
        (head / "engine" / "tools_ecs_bench").mkdir(parents=True)
        (head / "engine/tools_ecs_bench/Cargo.toml").write_text("[package]\n", encoding="utf-8")
        out_file = self.dir / "out.txt"
        code, _, _ = self.run_cli("presence", "--benchmark", "ecs", "--gates", self.gates, "--benchmarks", self.bench,
                                  "--base", base, "--head", head, "--github-output", out_file)
        self.assertEqual(code, 0)
        text = out_file.read_text(encoding="utf-8")
        self.assertIn("head=true", text)
        self.assertIn("base=false", text)

    def test_a_tool_deleted_by_the_push_is_red(self):
        base, head = self.trees(True)
        (head / "engine/tools_ecs_bench/Cargo.toml").unlink()
        code, out, err = self.run_cli("presence", "--benchmark", "ecs", "--gates", self.gates, "--benchmarks",
                                      self.bench, "--base", base, "--head", head)
        self.assertEqual(code, 1)
        self.assertIn("::error::", out + err)

    def test_compare_writes_the_ecs_summary_and_judges_with_the_same_bands(self):
        head, base = self.dir / "head.json", self.dir / "base.json"
        base.write_text(json.dumps({"count": 1000, "counts": [1000, 1000], "workload": WORK}), encoding="utf-8")
        head.write_text(json.dumps({"count": 1200, "counts": [1200, 1200], "workload": WORK}), encoding="utf-8")
        code, out, err = self.run_cli("compare", "--benchmark", "ecs", "--gates", self.gates, "--benchmarks", self.bench,
                                      "--head", head, "--base", base, "--summary", self.dir / "s.md")
        self.assertEqual(code, 1)
        self.assertIn("::error::", out + err)
        self.assertIn("ECS instruction count", (self.dir / "s.md").read_text(encoding="utf-8"))

    def test_measure_needs_the_ecs_table(self):
        self.bench.write_text(BENCH.split("[ecs_icount]")[0], encoding="utf-8")
        code, _, err = self.run_cli("measure", "--benchmark", "ecs", "--gates", self.gates, "--benchmarks", self.bench,
                                    "--bench", "x", "--out", self.dir / "o.json")
        self.assertEqual(code, 1)
        self.assertTrue(err)


if __name__ == "__main__":
    unittest.main()
