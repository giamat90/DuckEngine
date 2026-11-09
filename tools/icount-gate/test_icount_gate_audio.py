"""Tests of the audio profile of the instruction-count gate (card 5-15). The tool is the one of the physics gate
(card 4-23); a profile names the benchmark table of `gates/benchmarks.toml`, the workload flags of the benchmark
binary and the words of the summary. The audio profile counts the instructions of the voice scene of
`tools_audio_bench icount-workload`, base tree against pushed commit on the same runner, so the verdict does not
depend on which CPU type the runner has (wall time did: 19 % against 37 % of a core on hosted Ubuntu)."""

import io
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

[audio_icount]
package = "tools_audio_bench"
manifest = "engine/tools_audio_bench/Cargo.toml"
voices = 1024
blocks = 200
runs = 2
noise_max_percent = 0.5
"""
WORK = "voices 1024 blocks 200 checksum 7d56f729e6c05fb3\n"


def audio_config(bench=BENCH):
    with tempfile.TemporaryDirectory() as tmp:
        g, b = Path(tmp) / "gates.toml", Path(tmp) / "benchmarks.toml"
        g.write_text(GATES, encoding="utf-8")
        b.write_text(bench, encoding="utf-8")
        return ig.read_config(g, b, ig.AUDIO)


class ReadAudioConfig(unittest.TestCase):
    def test_reads_the_bands_and_the_voice_workload(self):
        cfg = audio_config()
        self.assertEqual((cfg.warn, cfg.fail), (5, 10))
        self.assertEqual((cfg.package, cfg.runs), ("tools_audio_bench", 2))
        self.assertEqual(cfg.manifest, Path("engine/tools_audio_bench/Cargo.toml"))
        self.assertEqual(cfg.workload, (("--voices", 1024), ("--blocks", 200)))

    def test_the_real_gates_file_reads_the_audio_profile(self):
        cfg = ig.read_config(REPO / "gates" / "gates.toml", REPO / "gates" / "benchmarks.toml", ig.AUDIO)
        self.assertEqual((cfg.warn, cfg.fail), (5, 10))
        self.assertEqual((cfg.package, cfg.runs), ("tools_audio_bench", 2))
        self.assertEqual(dict(cfg.workload)["--voices"], 1024)

    def test_the_default_profile_is_still_physics(self):
        cfg = ig.read_config(REPO / "gates" / "gates.toml", REPO / "gates" / "benchmarks.toml")
        self.assertEqual((cfg.package, cfg.rows, cfg.steps), ("tools_physics_bench", 20, 30))

    def test_a_missing_audio_table_is_an_error_not_a_fallback_to_physics(self):
        with self.assertRaises(ig.GateError):
            audio_config(BENCH.split("[audio_icount]")[0])

    def test_the_workload_must_be_sane(self):
        for old, new in (("voices = 1024", "voices = 0"), ("voices = 1024", "voices = 1000001"),
                         ("blocks = 200", "blocks = 0"), ("blocks = 200", "blocks = 1000001"),
                         ("runs = 2", "runs = 1"), ('package = "tools_audio_bench"', 'package = "a;b"')):
            with self.subTest(old=old, new=new), self.assertRaises(ig.GateError):
                audio_config(BENCH.replace(old, new))

    def test_the_noise_limit_of_the_audio_table_may_not_exceed_a_tenth_of_the_warn_band(self):
        audio = BENCH.split("[audio_icount]")[1].replace("noise_max_percent = 0.5", "noise_max_percent = 0.6")
        with self.assertRaises(ig.GateError):
            audio_config(BENCH.split("[audio_icount]")[0] + "[audio_icount]" + audio)


class MeasureAudio(unittest.TestCase):
    def test_runs_the_voice_workload_with_the_trusted_arguments(self):
        calls = []

        def run(argv, log_path):
            calls.append(argv)
            Path(log_path).write_text("==9== Collected : 5000\n", encoding="utf-8")
            return ig.Run(0, WORK)

        out = ig.measure("bin/tools_audio_bench", audio_config(), run)
        self.assertEqual(calls[0][-5:], ["icount-workload", "--voices", "1024", "--blocks", "200"])
        self.assertEqual(out["count"], 5000)
        self.assertEqual(out["workload"], WORK.strip())

    def test_the_physics_arguments_are_unchanged(self):
        calls = []

        def run(argv, log_path):
            calls.append(argv)
            Path(log_path).write_text("==9== Collected : 5000\n", encoding="utf-8")
            return ig.Run(0, "pairs 1\n")

        with tempfile.TemporaryDirectory() as tmp:
            g, b = Path(tmp) / "gates.toml", Path(tmp) / "benchmarks.toml"
            g.write_text(GATES, encoding="utf-8")
            b.write_text(BENCH, encoding="utf-8")
            ig.measure("bin/x", ig.read_config(g, b), run)
        self.assertEqual(calls[0][-5:], ["icount-workload", "--rows", "20", "--steps", "30"])


class AudioWords(unittest.TestCase):
    def test_the_skip_line_names_the_audio_benchmark(self):
        with tempfile.TemporaryDirectory() as tmp:
            base, head = Path(tmp) / "base", Path(tmp) / "head"
            base.mkdir()
            head.mkdir()
            state, message = ig.presence(base, head, Path("engine/tools_audio_bench/Cargo.toml"), ig.AUDIO.skip)
        self.assertEqual(state, "skip")
        self.assertTrue(message.startswith("not run: audio benchmark tool absent"), message)

    def test_the_summary_names_the_audio_workload_and_not_physics(self):
        cfg = audio_config()
        head = {"count": 1070, "counts": [1070, 1070], "workload": WORK}
        base = {"count": 1000, "counts": [1000, 1000], "workload": WORK}
        text = ig.render_summary(ig.classify(1000, 1070, 5, 10), base, head, cfg)
        self.assertIn("Audio instruction count", text)
        self.assertIn("--voices 1024 --blocks 200", text)
        self.assertNotIn("Physics", text)

    def test_the_physics_summary_keeps_its_words(self):
        with tempfile.TemporaryDirectory() as tmp:
            g, b = Path(tmp) / "gates.toml", Path(tmp) / "benchmarks.toml"
            g.write_text(GATES, encoding="utf-8")
            b.write_text(BENCH, encoding="utf-8")
            cfg = ig.read_config(g, b)
        head = {"count": 1000, "counts": [1000, 1000], "workload": "pairs 1"}
        text = ig.render_summary(ig.classify(1000, 1000, 5, 10), head, head, cfg)
        self.assertIn("Physics instruction count (tools_physics_bench icount-workload --rows 20 --steps 30", text)


class AudioCli(unittest.TestCase):
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

    def test_presence_for_audio_skips_with_the_audio_line_and_sets_the_audio_package(self):
        base, head = self.dir / "base", self.dir / "head"
        for tree in (base, head):
            tree.mkdir()
        out_file, summary = self.dir / "out.txt", self.dir / "sum.md"
        code, _, _ = self.run_cli("presence", "--benchmark", "audio", "--gates", self.gates, "--benchmarks", self.bench,
                                  "--base", base, "--head", head, "--github-output", out_file, "--summary", summary)
        self.assertEqual(code, 0)
        text = summary.read_text(encoding="utf-8")
        self.assertIn("Audio instruction count: not run: audio benchmark tool absent", text)
        self.assertIn("package=tools_audio_bench", out_file.read_text(encoding="utf-8"))

    def test_presence_for_audio_compares_when_both_trees_have_the_audio_tool(self):
        base, head = self.dir / "base", self.dir / "head"
        for tree in (base, head):
            (tree / "engine" / "tools_audio_bench").mkdir(parents=True)
            (tree / "engine/tools_audio_bench/Cargo.toml").write_text("[package]\n", encoding="utf-8")
        out_file = self.dir / "out.txt"
        code, _, _ = self.run_cli("presence", "--benchmark", "audio", "--gates", self.gates, "--benchmarks", self.bench,
                                  "--base", base, "--head", head, "--github-output", out_file)
        self.assertEqual(code, 0)
        text = out_file.read_text(encoding="utf-8")
        self.assertIn("head=true", text)
        self.assertIn("base=true", text)

    def test_compare_for_audio_writes_the_audio_summary_and_judges_with_the_same_bands(self):
        import json
        head, base = self.dir / "head.json", self.dir / "base.json"
        base.write_text(json.dumps({"count": 1000, "counts": [1000, 1000], "workload": WORK}), encoding="utf-8")
        head.write_text(json.dumps({"count": 1200, "counts": [1200, 1200], "workload": WORK}), encoding="utf-8")
        code, out, err = self.run_cli("compare", "--benchmark", "audio", "--gates", self.gates, "--benchmarks", self.bench,
                                      "--head", head, "--base", base, "--summary", self.dir / "s.md")
        self.assertEqual(code, 1)
        self.assertIn("::error::", out + err)
        self.assertIn("Audio instruction count", (self.dir / "s.md").read_text(encoding="utf-8"))

    def test_an_unknown_benchmark_is_refused_by_the_parser(self):
        with self.assertRaises(SystemExit), redirect_stderr(io.StringIO()):
            ig.main(["presence", "--benchmark", "video", "--gates", "g", "--benchmarks", "b", "--base", "x", "--head", "y"])

    def test_measure_for_audio_needs_the_audio_table(self):
        self.bench.write_text(BENCH.split("[audio_icount]")[0], encoding="utf-8")
        code, _, err = self.run_cli("measure", "--benchmark", "audio", "--gates", self.gates, "--benchmarks", self.bench,
                                    "--bench", "x", "--out", self.dir / "o.json")
        self.assertEqual(code, 1)
        self.assertTrue(err)


if __name__ == "__main__":
    unittest.main()
