#!/usr/bin/env python3
"""The D2 golden hash checker (card 4-20).

Compares the `GOLDEN|...` lines that `cargo test -p physics --features cross-platform-determinism
--test determinism -- golden:: --nocapture` prints with `gates/golden/physics-replay.toml`, decides whether the
`d2_determinism` job runs at all, and checks the build flags. It runs from a checkout of `main`
in CI (rules-digest section 8): the golden file and this tool are never taken from the pushed tree.

Subcommands:
  lint      the golden file is well formed
  verify    a run equals the golden file (exit 1 with the first differing scene and step otherwise)
  record    write the golden file from a run (needs --i-am-changing-the-golden-file: a governance change)
  presence  run the job, skip it ("not run: physics crate absent") or fail it
  flags     no target-cpu / fma flag reaches the build
"""

import argparse
import re
import sys
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

FORMAT = 1
FEATURE = "cross-platform-determinism"
HASH = re.compile(r"[0-9a-f]{16}")
GATED = {("windows", "x86_64"), ("linux", "x86_64")}
CRATE_MANIFEST = Path("engine/physics/Cargo.toml")
RECORDER = Path("engine/physics/tests/determinism/golden.rs")
MAX_STEPS = 1_000_000
SKIP_MESSAGE = "not run: physics crate absent (engine/physics/Cargo.toml is on neither main nor the pushed commit)"


class GoldenError(Exception):
    """The output or the golden file is not what the format says."""


@dataclass
class Scene:
    steps: int
    stride: int
    hashes: dict = field(default_factory=dict)


@dataclass
class Run:
    format: int
    feature: str
    os: str
    arch: str
    fma: bool
    scenes: dict


@dataclass
class Golden:
    format: int
    feature: str
    scenes: dict


def checkpoints(steps, stride):
    """The steps that carry a hash: the first, every stride-th, the last."""
    return sorted({1, steps} | set(range(stride, steps + 1, stride)))


def _int(value, what):
    if not re.fullmatch(r"0|[1-9][0-9]*", value):
        raise GoldenError(f"{what} is not a number: {value!r}")
    return int(value)


def _check_scene(name, scene):
    if not (1 <= scene.steps <= MAX_STEPS and 1 <= scene.stride <= MAX_STEPS):
        raise GoldenError(f"scene {name!r}: steps and stride must be between 1 and {MAX_STEPS}")
    if sorted(scene.hashes) != checkpoints(scene.steps, scene.stride):
        raise GoldenError(f"scene {name!r}: the checkpoints are not step 1, every {scene.stride}th step and step {scene.steps}")


def parse_output(output):
    """The run described by the GOLDEN lines of `output`; other lines are ignored."""
    lines = [line.rstrip("\r") for line in output.split("\n")]
    golden_lines = [line for line in lines if line.startswith("GOLDEN|")]
    if not golden_lines:
        raise GoldenError("no GOLDEN line in the output (the test did not run or printed nothing)")
    header = {}
    scenes = {}
    end = None
    for index, line in enumerate(golden_lines):
        if end is not None:
            raise GoldenError(f"GOLDEN lines after the end line: {line!r}")
        fields = line.split("|")[1:]
        kind = fields[0]
        if kind in ("format", "fma") and len(fields) == 2:
            header[kind] = fields[1]
        elif kind == "feature" and len(fields) == 3:
            if fields[1] != FEATURE:
                raise GoldenError(f"unknown feature {fields[1]!r}")
            header["feature"] = fields[2]
        elif kind == "platform" and len(fields) == 3:
            header["os"], header["arch"] = fields[1], fields[2]
        elif kind == "scene" and len(fields) == 4:
            if fields[1] in scenes:
                raise GoldenError(f"scene {fields[1]!r} twice")
            scenes[fields[1]] = Scene(_int(fields[2], "steps"), _int(fields[3], "stride"))
        elif kind == "hash" and len(fields) == 4:
            _add_hash(scenes, fields)
        elif kind == "end" and len(fields) == 2:
            end = (_int(fields[1], "end count"), index)
        else:
            raise GoldenError(f"unknown or malformed GOLDEN line: {line!r}")
    if end is None:
        raise GoldenError("no end line: the output is cut short")
    if end[0] != end[1]:
        raise GoldenError(f"the end line counts {end[0]} lines, {end[1]} were printed before it (count mismatch)")
    return _run(header, scenes)


def _add_hash(scenes, fields):
    _, name, step, value = fields
    if name not in scenes:
        raise GoldenError(f"hash of scene {name!r} that has no scene line")
    if not HASH.fullmatch(value):
        raise GoldenError(f"hash of {name!r} step {step} is not 16 lowercase hex digits: {value!r}")
    step = _int(step, "step")
    if step in scenes[name].hashes:
        raise GoldenError(f"scene {name!r} step {step} twice")
    scenes[name].hashes[step] = value


def _run(header, scenes):
    missing = {"format", "feature", "os", "arch", "fma"} - set(header)
    if missing:
        raise GoldenError(f"header line missing: {', '.join(sorted(missing))}")
    if _int(header["format"], "format") != FORMAT:
        raise GoldenError(f"format {header['format']} is not the format {FORMAT} this tool reads")
    if header["fma"] not in ("true", "false"):
        raise GoldenError(f"fma must be true or false, not {header['fma']!r}")
    if header["feature"] not in ("on", "off"):
        raise GoldenError(f"feature must be on or off, not {header['feature']!r}")
    for name, scene in scenes.items():
        _check_scene(name, scene)
    return Run(FORMAT, header["feature"], header["os"], header["arch"], header["fma"] == "true", scenes)


def environment_problems(run):
    """Why a run cannot be compared at all: wrong feature, fused operations, a platform that is not gated."""
    problems = []
    if run.feature != "on":
        problems.append(f"feature {FEATURE} is {run.feature}: the golden file is taken with it on")
    if run.fma:
        problems.append("the build has target_feature fma: not the build the golden file is taken with")
    if (run.os, run.arch) not in GATED:
        problems.append(f"platform {run.os} {run.arch} is not gated (Windows and Linux x86-64 only)")
    return problems


def parse_golden(data):
    """The golden file as a `Golden`; raises GoldenError when malformed."""
    try:
        doc = tomllib.loads(data)
    except tomllib.TOMLDecodeError as error:
        raise GoldenError(f"the golden file is not valid TOML: {error}") from error
    if doc.get("format") != FORMAT:
        raise GoldenError(f"golden format {doc.get('format')!r} is not {FORMAT}")
    if doc.get("feature") != FEATURE:
        raise GoldenError(f"golden feature {doc.get('feature')!r} is not {FEATURE!r}")
    scenes = {}
    entries = doc.get("scene", [])
    if not isinstance(entries, list):
        raise GoldenError("'scene' must be an array of tables")
    for entry in entries:
        scene = _golden_scene(entry)
        if entry["name"] in scenes:
            raise GoldenError(f"golden scene {entry['name']!r} twice")
        scenes[entry["name"]] = scene
    if not scenes:
        raise GoldenError("the golden file has no scene")
    return Golden(FORMAT, FEATURE, scenes)


def _golden_scene(entry):
    if not isinstance(entry, dict):
        raise GoldenError("a golden scene is not a table")
    for key in ("name", "steps", "stride", "hash"):
        if key not in entry:
            raise GoldenError(f"a golden scene lacks {key!r}")
    numbers = (entry["steps"], entry["stride"])
    if not isinstance(entry["name"], str) or not isinstance(entry["hash"], dict) or not all(
            type(n) is int for n in numbers):
        raise GoldenError(f"golden scene {entry['name']!r}: name must be text, steps and stride integers, hash a table")
    hashes = {}
    for key, value in entry["hash"].items():
        if not isinstance(value, str) or not HASH.fullmatch(value):
            raise GoldenError(f"golden hash of {entry['name']!r} step {key} is not 16 lowercase hex digits")
        hashes[_int(key, "golden step")] = value
    scene = Scene(entry["steps"], entry["stride"], hashes)
    _check_scene(entry["name"], scene)
    return scene


def render_golden(run, toolchain, recorded):
    """The golden file text for `run`: LF line endings, scenes in the order of the run."""
    out = [
        "# D2 golden hashes of the physics crate (card 4-20). Protected path (gates/): a change is a governance change.",
        "# State hash (FNV-1a 64 bit, World::state_hash) after the listed steps of each scene, scalar serial reference,",
        f"# feature {FEATURE} on. Regenerate only in a governance task: see wiki/architecture/physics.md section 3.",
        f"format = {FORMAT}",
        f'feature = "{FEATURE}"',
        f'toolchain = "{toolchain}"',
        f'recorded = "{recorded}"',
        'recorder = "cargo test -p physics --features cross-platform-determinism --test determinism -- golden:: --nocapture"',
        "",
    ]
    for name, scene in run.scenes.items():
        out += ["[[scene]]", f'name = "{name}"', f"steps = {scene.steps}", f"stride = {scene.stride}", "[scene.hash]"]
        out += [f'{step} = "{scene.hashes[step]}"' for step in sorted(scene.hashes)]
        out.append("")
    return "\n".join(out).rstrip("\n") + "\n"


def compare(golden, run):
    """One problem per scene that differs (the first differing step), plus missing and extra scenes."""
    problems = []
    for name, expected in golden.scenes.items():
        got = run.scenes.get(name)
        if got is None:
            problems.append(f"scene {name!r} is missing from the run")
            continue
        problems += _compare_scene(name, expected, got)
    for name in run.scenes:
        if name not in golden.scenes:
            problems.append(f"scene {name!r} is in the run but not in the golden file")
    return problems


def _compare_scene(name, expected, got):
    if (expected.steps, expected.stride) != (got.steps, got.stride):
        return [f"scene {name!r}: steps/stride {got.steps}/{got.stride}, the golden file has {expected.steps}/{expected.stride}"]
    for step in sorted(expected.hashes):
        if got.hashes.get(step) != expected.hashes[step]:
            return [f"scene {name!r}: first difference at step {step}: got {got.hashes.get(step)}, golden {expected.hashes[step]}"]
    return []


def table(golden, run, problems):
    """A Markdown table for the job summary."""
    rows = ["| scene | steps | checkpoints | result |", "|---|---|---|---|"]
    for name, expected in golden.scenes.items():
        bad = [p for p in problems if f"scene {name!r}" in p]
        rows.append(f"| {name} | {expected.steps} | {len(expected.hashes)} | {'DIFFERS: ' + bad[0].split(': ', 1)[-1] if bad else 'equal'} |")
    heading = f"### D2 golden hashes on {run.os} {run.arch}: {'RED' if problems else 'equal'}"
    return "\n".join([heading, "", *rows, ""])


def presence(trusted, head):
    """('run' | 'skip' | 'fail', message). Skipped only when neither main nor the pushed commit has the crate."""
    in_head = (Path(head) / CRATE_MANIFEST).is_file()
    in_trusted = (Path(trusted) / CRATE_MANIFEST).is_file()
    if in_head:
        if not (Path(head) / RECORDER).is_file():
            return "fail", f"the physics crate is present but {RECORDER.as_posix()} (the golden recorder) is missing: not a skip"
        return "run", "physics crate present"
    if in_trusted:
        return "fail", "the physics crate exists on main but the pushed commit has deleted or moved it: not a skip"
    return "skip", SKIP_MESSAGE


BAD_CPU = re.compile(r"target-cpu")
BAD_FMA = re.compile(r"target-feature[=\s\x1f]*[^\"'\s]*\+fma|\+fma\b")


def _flag_problem(where, value):
    text = value.replace("\x1f", " ")
    if BAD_CPU.search(text):
        return f"{where} sets target-cpu: {value!r}"
    if BAD_FMA.search(text) or re.search(r"target-feature[=\s]*[^\"'\s]*\+(fma|fma3)", text):
        return f"{where} enables fma: {value!r}"
    return None


def flag_problems(env, configs):
    """Problems in the flag variables of `env` and in the text of Cargo config files."""
    # The cfg checks (compile_error! in base, the fma line of the recorder) are the authoritative backstop; this
    # is the early, readable failure.
    names = [name for name in env if name in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS", "RUSTDOCFLAGS")
             or re.fullmatch(r"CARGO_TARGET_.*_RUSTFLAGS", name)]
    found = [_flag_problem(name, env[name]) for name in names]
    found += [_flag_problem("a Cargo config file", config) for config in configs]
    return [problem for problem in found if problem]


def _cargo_configs(repo):
    import os
    roots = [Path(repo) / ".cargo", Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))]
    files = [root / name for root in roots for name in ("config.toml", "config")]
    return [path.read_text(encoding="utf-8", errors="replace") for path in files if path.is_file()]


def _read(path, what):
    try:
        return Path(path).read_text(encoding="utf-8", errors="replace")
    except OSError as error:
        raise GoldenError(f"cannot read {what} {path}: {error.strerror or error}") from error


def _fail(message):
    print(f"golden-check: {message}", file=sys.stderr)
    return 1


def _append(path, text):
    if path:
        with open(path, "a", encoding="utf-8", newline="\n") as handle:
            handle.write(text)


def cmd_lint(args):
    golden = parse_golden(_read(args.golden, "the golden file"))
    print(f"golden file ok: {len(golden.scenes)} scenes, {sum(len(s.hashes) for s in golden.scenes.values())} hashes")
    return 0


def cmd_verify(args):
    golden = parse_golden(_read(args.golden, "the golden file"))
    run = parse_output(_read(args.output, "the test output"))
    problems = environment_problems(run) + compare(golden, run)
    _append(args.summary, table(golden, run, problems))
    for problem in problems:
        print(f"golden-check: {problem}", file=sys.stderr)
    if problems:
        return 1
    print(f"D2 golden hashes equal on {run.os} {run.arch}: {len(golden.scenes)} scenes")
    return 0


def cmd_record(args):
    if not args.i_am_changing_the_golden_file:
        return _fail("refusing to write the golden file: a change of physics results is a governance change the owner sees; "
                     "pass --i-am-changing-the-golden-file in a governance task")
    run = parse_output(_read(args.output, "the test output"))
    problems = environment_problems(run)
    if problems:
        return _fail("; ".join(problems))
    toolchain = args.toolchain or "unknown"
    recorded = args.recorded or f"{run.os} {run.arch}"
    Path(args.golden).parent.mkdir(parents=True, exist_ok=True)
    with open(args.golden, "w", encoding="utf-8", newline="\n") as handle:
        handle.write(render_golden(run, toolchain, recorded))
    print(f"wrote {args.golden}: {len(run.scenes)} scenes")
    return 0


def cmd_presence(args):
    state, message = presence(args.trusted, args.head)
    if state == "fail":
        return _fail(message)
    if state == "skip":
        print(f"::notice::{message}")
        _append(args.summary, f"### D2 golden hashes: {message}\n")
    _append(args.github_output, f"present={'true' if state == 'run' else 'false'}\n")
    return 0


def cmd_flags(args):
    import os
    problems = flag_problems(dict(os.environ), _cargo_configs(args.repo))
    for problem in problems:
        print(f"golden-check: {problem}", file=sys.stderr)
    return 1 if problems else 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    lint = sub.add_parser("lint")
    lint.add_argument("--golden", required=True)
    verify = sub.add_parser("verify")
    verify.add_argument("--golden", required=True)
    verify.add_argument("--output", required=True)
    verify.add_argument("--summary")
    record = sub.add_parser("record")
    record.add_argument("--golden", required=True)
    record.add_argument("--output", required=True)
    record.add_argument("--toolchain")
    record.add_argument("--recorded")
    record.add_argument("--i-am-changing-the-golden-file", action="store_true")
    pres = sub.add_parser("presence")
    pres.add_argument("--trusted", required=True)
    pres.add_argument("--head", required=True)
    pres.add_argument("--github-output")
    pres.add_argument("--summary")
    flags = sub.add_parser("flags")
    flags.add_argument("--repo", required=True)
    args = parser.parse_args(argv)
    commands = {"lint": cmd_lint, "verify": cmd_verify, "record": cmd_record, "presence": cmd_presence, "flags": cmd_flags}
    try:
        return commands[args.command](args)
    except GoldenError as error:
        return _fail(str(error))


if __name__ == "__main__":
    sys.exit(main())
