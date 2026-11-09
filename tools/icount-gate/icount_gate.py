#!/usr/bin/env python3
"""The instruction-count gate (physics, card 4-23; audio, card 5-15; rules-digest section 7: "PR benchmark gate on instruction
counts (warn 5%, fail 10%)"). A profile names the benchmark table, the workload flags and the words.

What is measured comes from the trees under test: the `tools_physics_bench` binary built from the base tree and
from the pushed commit. What is decided comes from here, run from a checkout of `main` (digest section 8): the
bands are read from `gates/gates.toml [benchmark]` and the workload arguments and the noise limit from
`gates/benchmarks.toml` of `main`, the
count is read from valgrind's own log file (`--log-file`, so nothing the measured program prints can pose as a
count), and the percentages are computed with integers.

Subcommands:
  presence  decide from the two trees whether to compare, record only, skip (stated) or fail
  measure   run the workload under `valgrind --tool=callgrind` N times and write the count as JSON
  compare   base JSON (optional) against head JSON: the verdict, the summary table, the exit code

Exit codes: 0 pass, warn, record or stated skip; 1 fail band, a missing or unreadable count, or a bad setup.
"""

import argparse
import json
import re
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import dataclass
from pathlib import Path

WORKLOAD_KEEP = 2000
IDENTIFIER = re.compile(r"[A-Za-z0-9_]+(?:-[A-Za-z0-9_]+)*")
COLLECTED = re.compile(r"^==\d+==\s+Collected\s*:\s*(\d+)\b", re.M)
SKIP_MESSAGE = ("not run: physics benchmark tool absent (engine/tools_physics_bench/Cargo.toml is in neither "
                "the base tree nor the pushed commit)")
NOISE_DIVISOR = 10  # the noise limit is at most a tenth of the warn band (card 4-22, gate_feasibility)
MAX_ROWS = 255
MAX_WORKLOAD = 1_000_000


class GateError(Exception):
    """The setup, a measurement or a result file is not what the gate needs."""


@dataclass(frozen=True)
class Profile:
    name: str
    title: str
    table: str
    skip: str
    keys: tuple  # (key, flag, high): the workload arguments in the order they are passed


PHYSICS = Profile("physics", "Physics", "physics_icount", SKIP_MESSAGE,
                  (("rows", "--rows", MAX_ROWS), ("steps", "--steps", MAX_WORKLOAD)))
AUDIO = Profile("audio", "Audio", "audio_icount",
                "not run: audio benchmark tool absent (engine/tools_audio_bench/Cargo.toml is in neither "
                "the base tree nor the pushed commit)",
                (("voices", "--voices", MAX_WORKLOAD), ("blocks", "--blocks", MAX_WORKLOAD)))
PROFILES = {profile.name: profile for profile in (PHYSICS, AUDIO)}


@dataclass(frozen=True)
class Config:
    warn: int
    fail: int
    package: str
    manifest: Path
    workload: tuple  # ((flag, value), ...)
    runs: int
    noise_max_percent: float
    title: str = "Physics"

    @property
    def rows(self):  # physics profile only: an audio config has no --rows
        return dict(self.workload)["--rows"]

    @property
    def steps(self):  # physics profile only
        return dict(self.workload)["--steps"]

    def workload_args(self):
        return [part for flag, value in self.workload for part in (flag, str(value))]


@dataclass(frozen=True)
class Verdict:
    level: str  # pass | warn | fail | record
    delta_text: str
    message: str


@dataclass(frozen=True)
class Run:
    returncode: int
    stdout: str


def _int_in(table, key, low, high):
    value = table.get(key)
    if isinstance(value, bool) or not isinstance(value, int) or not low <= value <= high:
        raise GateError(f"gates.toml or benchmarks.toml: {key} must be an integer between {low} and {high}")
    return value


def _manifest(value):
    path = Path(value) if isinstance(value, str) else None
    if path is None or path.is_absolute() or ".." in path.parts or not path.parts or value.startswith(("/", "\\")):
        raise GateError("benchmarks.toml: manifest must be a relative path inside the tree")
    return path


def _toml(path, key):
    try:
        with open(path, "rb") as handle:
            return tomllib.load(handle)[key]
    except (OSError, KeyError, tomllib.TOMLDecodeError) as error:
        raise GateError(f"cannot read [{key}] in {path}: {error!r}") from error


def read_config(gates, benchmarks, profile=PHYSICS):
    """The bands from gates.toml [benchmark], the registered benchmark from the profile's benchmarks.toml table."""
    bench = _toml(gates, "benchmark")
    table = _toml(benchmarks, profile.table)
    warn, fail = _int_in(bench, "warn_percent", 1, 1000), _int_in(bench, "fail_percent", 1, 1000)
    if warn >= fail:
        raise GateError("gates.toml: warn_percent must be below fail_percent")
    package = table.get("package")
    if not isinstance(package, str) or not IDENTIFIER.fullmatch(package):
        raise GateError("benchmarks.toml: package must be a plain crate name")
    noise = table.get("noise_max_percent")
    if isinstance(noise, bool) or not isinstance(noise, (int, float)) or not 0 <= noise * NOISE_DIVISOR <= warn:
        raise GateError("benchmarks.toml: noise_max_percent must be between 0 and a tenth of warn_percent")
    if abs(noise * 10 - round(noise * 10)) > 1e-9:
        raise GateError("benchmarks.toml: noise_max_percent must be a multiple of 0.1")
    workload = tuple((flag, _int_in(table, key, 1, high)) for key, flag, high in profile.keys)
    return Config(warn, fail, package, _manifest(table.get("manifest")), workload,
                  _int_in(table, "runs", 2, 20), float(noise), profile.title)


def parse_callgrind(text):
    """The one `==pid== Collected : N` line of a valgrind log. None, several (one may be forged by the measured
    program, which can append to the log file named on its own command line) or N = 0 is an error."""
    found = COLLECTED.findall(text)
    if len(found) != 1 or int(found[0]) == 0:
        raise GateError("the valgrind log must have exactly one instruction count (`==pid== Collected : N`, N above 0)")
    return int(found[0])


def check_spread(counts, noise_max_percent):
    """The runs of one tree must agree within the noise limit, or the bands cannot be read."""
    low, high = min(counts), max(counts)
    if (high - low) * 1000 > round(noise_max_percent * 10) * low:  # integers: the limit is a multiple of 0.1 %
        raise GateError(f"the {len(counts)} runs differ by more than {noise_max_percent}%: {counts}")


def _percent_text(base, head):
    hundredths = (abs(head - base) * 10_000 + base // 2) // base
    sign = "-" if head < base else "+"
    return f"{sign}{hundredths // 100}.{hundredths % 100:02d}%"


def classify(base, head, warn, fail):
    """Strictly above `warn` percent warns, strictly above `fail` percent fails; a lower count never fails."""
    if base <= 0 or head <= 0:
        raise GateError("both instruction counts must be positive")
    text = _percent_text(base, head)
    over = (head - base) * 100
    if over > fail * base:
        return Verdict("fail", text, f"instruction count rose {text}, above the fail band of {fail}%")
    if over > warn * base:
        return Verdict("warn", text, f"instruction count rose {text}, above the warn band of {warn}%")
    note = " (an improvement)" if head < base else ""
    return Verdict("pass", text, f"instruction count changed {text}, within the warn band of {warn}%{note}")


def record_only(head):
    return Verdict("record", "n/a", f"no baseline, recorded only: {head} instructions (the base tree has no benchmark tool)")


def default_runner(argv, log_path):
    done = subprocess.run(argv, capture_output=True, text=True, check=False, stdin=subprocess.DEVNULL)  # noqa: S603
    return Run(done.returncode, done.stdout)


def measure(bench, cfg, runner=default_runner):
    """Run the workload `cfg.runs` times under callgrind; the result is a dict ready for JSON."""
    counts, works = [], []
    with tempfile.TemporaryDirectory() as tmp:
        for index in range(cfg.runs):
            log = Path(tmp) / f"callgrind{index}.log"
            argv = ["valgrind", "--tool=callgrind", "--callgrind-out-file=/dev/null", f"--log-file={log}",
                    str(bench), "icount-workload", *cfg.workload_args()]
            done = runner(argv, log)
            if done.returncode != 0:
                raise GateError(f"run {index + 1}: valgrind or the workload failed (exit {done.returncode})")
            if not done.stdout.strip():
                raise GateError(f"run {index + 1}: the workload printed nothing, so it proves no work was done")
            works.append(done.stdout.strip())
            counts.append(parse_callgrind(log.read_text(encoding="utf-8", errors="replace")))
    if len(set(works)) != 1:
        raise GateError("the runs did different work (the workload output differs between runs)")
    check_spread(counts, cfg.noise_max_percent)
    return {"count": min(counts), "counts": counts, "workload": works[0][:WORKLOAD_KEEP]}


def presence(base, head, manifest, skip_message=SKIP_MESSAGE):
    """('compare' | 'record' | 'skip' | 'fail', message). A tool that the base has and the push lacks is never a skip."""
    in_head = (Path(head) / manifest).is_file()
    in_base = (Path(base) / manifest).is_file()
    if in_head and in_base:
        return "compare", "the benchmark tool is in both trees"
    if in_head:
        return "record", "the base tree has no benchmark tool: no baseline, recorded only"
    if in_base:
        return "fail", f"{manifest.as_posix()} is in the base tree but the pushed commit has deleted or moved it: not a skip"
    return "skip", skip_message


def render_summary(verdict, base, head, cfg):
    args = " ".join(cfg.workload_args())
    lines = [f"### {cfg.title} instruction count ({cfg.package} icount-workload {args}, "
             f"valgrind callgrind, serial)", "",
             f"Bands from `gates/gates.toml` of main: warn above {cfg.warn}%, fail above {cfg.fail}%.", "",
             "| tree | instructions | runs |", "|---|---|---|"]
    if base is not None:
        lines.append(f"| base | {base['count']} | {base['counts']} |")
    lines.append(f"| head | {head['count']} | {head['counts']} |")
    lines += ["", f"**{verdict.level}** ({verdict.delta_text}): {verdict.message}"]
    if base is not None and base.get("workload") != head.get("workload"):
        lines += ["", "Note: the workload output differs between base and head (the work counters or the replay hash "
                      "changed), so a different count is expected; read both outputs below.",
                  "", "base output:", "```", base.get("workload", ""), "```"]
    if base is not None and base.get("workload") != head.get("workload") and verdict.delta_text.startswith("-"):
        lines += ["", "Warning: the count fell while the workload output changed: work was removed? Read the diff."]
    lines += ["", "head output:", "```", head.get("workload", ""), "```", ""]
    return "\n".join(lines)


def _fail(message):
    print(f"::error::icount-gate: {message}")
    print(f"icount-gate: {message}", file=sys.stderr)
    return 1


def _append(path, text):
    if path:
        with open(path, "a", encoding="utf-8", newline="\n") as handle:
            handle.write(text)


def _load(path):
    try:
        data = json.loads(Path(path).read_text(encoding="utf-8"))
        count = data["count"]
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise GateError(f"cannot read the result {path}: {error!r}") from error
    if isinstance(count, bool) or not isinstance(count, int) or count <= 0:
        raise GateError(f"the result {path} has no positive count")
    return data


def cmd_presence(args):
    profile = PROFILES[args.benchmark]
    cfg = read_config(args.gates, args.benchmarks, profile)
    state, message = presence(args.base, args.head, cfg.manifest, profile.skip)
    if state == "fail":
        return _fail(message)
    if state == "skip":
        print(f"::notice::{message}")
        _append(args.summary, f"### {cfg.title} instruction count: {message}\n")
    elif state == "record":
        print(f"::notice::{message}")
    _append(args.github_output, f"head={'true' if state in ('compare', 'record') else 'false'}\n"
                                f"base={'true' if state == 'compare' else 'false'}\n"
                                f"package={cfg.package}\n")
    return 0


def cmd_measure(args):
    cfg = read_config(args.gates, args.benchmarks, PROFILES[args.benchmark])
    result = measure(args.bench, cfg, default_runner)
    Path(args.out).write_text(json.dumps(result), encoding="utf-8")
    print(f"{args.bench}: {result['count']} instructions, runs {result['counts']}")
    return 0


def cmd_compare(args):
    cfg = read_config(args.gates, args.benchmarks, PROFILES[args.benchmark])
    head = _load(args.head)
    base = _load(args.base) if args.base else None
    verdict = classify(base["count"], head["count"], cfg.warn, cfg.fail) if base else record_only(head["count"])
    _append(args.summary, render_summary(verdict, base, head, cfg))
    print(f"icount-gate: {verdict.level} {verdict.delta_text}: {verdict.message}")
    if verdict.level == "warn":
        print(f"::warning::icount-gate: {verdict.message}")
    if verdict.level == "fail":
        return _fail(verdict.message)
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    pres = sub.add_parser("presence")
    pres.add_argument("--benchmark", choices=sorted(PROFILES), default="physics")
    pres.add_argument("--gates", required=True)
    pres.add_argument("--benchmarks", required=True)
    pres.add_argument("--base", required=True)
    pres.add_argument("--head", required=True)
    pres.add_argument("--github-output")
    pres.add_argument("--summary")
    meas = sub.add_parser("measure")
    meas.add_argument("--benchmark", choices=sorted(PROFILES), default="physics")
    meas.add_argument("--gates", required=True)
    meas.add_argument("--benchmarks", required=True)
    meas.add_argument("--bench", required=True)
    meas.add_argument("--out", required=True)
    comp = sub.add_parser("compare")
    comp.add_argument("--benchmark", choices=sorted(PROFILES), default="physics")
    comp.add_argument("--gates", required=True)
    comp.add_argument("--benchmarks", required=True)
    comp.add_argument("--head", required=True)
    comp.add_argument("--base")
    comp.add_argument("--summary")
    args = parser.parse_args(argv)
    commands = {"presence": cmd_presence, "measure": cmd_measure, "compare": cmd_compare}
    try:
        return commands[args.command](args)
    except GateError as error:
        return _fail(str(error))


if __name__ == "__main__":
    sys.exit(main())
