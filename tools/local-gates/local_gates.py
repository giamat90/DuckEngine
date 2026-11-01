"""Run the local gates of a card in one command (card 1-39).

Under the develop-branch workflow (delivery-pipeline) the evidence for an ordinary card is a local gate run,
because `ci` no longer runs on every branch push. This runs the same commands as `ci`: Rust format, Clippy and
tests, the gate tools on the working tree, the unit tests of every Python tool, cargo-deny when installed, the
diff hash, and with --wsl the Linux test run through WSL. Every step runs; the exit code is 1 if one failed.

    local_gates.py [--repo .] [--base origin/main] [--wsl] [--list]

Not run locally (reported at the end): Miri, coverage, mutation, benchmarks, the metrics jobs. They run in the
`ci` checkpoints and in the develop-to-main PR. The gate tools here run from the working tree, not from a trusted
checkout of `main`: the CI run keeps that guarantee, this one is the early warning.
"""

import argparse
import os
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

MISSING = 127
NOT_RUN = "Miri, coverage, mutation, benchmarks, metrics, cargo-deny when not installed"
WSL_TARGET = "/root/target-gates"


@dataclass(frozen=True)
class Step:
    name: str
    cwd: Path
    argv: list


@dataclass(frozen=True)
class Result:
    name: str
    status: str


def _cargo(name, *args):
    return Step(name, Path("."), ["cargo", *args])


def rust_steps():
    return [
        _cargo("fmt", "fmt", "--all", "--check"),
        _cargo("clippy", "clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"),
        _cargo("build", "build", "--workspace", "--locked"),
        _cargo("test", "test", "--workspace", "--locked"),
        _cargo("clippy app_runtime without mimalloc", "clippy", "-p", "app_runtime", "--no-default-features", "--all-targets", "--locked", "--", "-D", "warnings"),
        _cargo("test app_runtime without mimalloc", "test", "-p", "app_runtime", "--no-default-features", "--locked"),
        _cargo("clippy base with profiling", "clippy", "-p", "base", "--features", "profiling", "--all-targets", "--locked", "--", "-D", "warnings"),
        _cargo("test base with profiling", "test", "-p", "base", "--features", "profiling", "--locked"),
    ]


def gate_steps(base):
    def py(name, *args):
        return Step(name, Path("."), ["python", *args])

    return [
        py("architecture", "tools/arch-check/arch_check.py", "--config", "gates/architecture.toml", "--repo", "."),
        py("unsafe budget", "tools/unsafe-budget/unsafe_budget.py", "--config", "gates/unsafe-budget.toml", "--architecture", "gates/architecture.toml", "--repo", "."),
        py("wiki lint", "tools/wiki-lint/wiki_lint.py", "--config", "gates/wiki-lint.toml", "--gates", "gates/gates.toml", "--repo", "."),
        py("digest", "tools/gate-check/check_digest.py", "--digest", "wiki/rules-digest.md", "--config", "gates/gates.toml", "--claude-md", "CLAUDE.md"),
        py("diff hash", "tools/diff-hash/diff_hash.py", "compute", "--base", base, "--head", "HEAD"),
    ]


def python_test_steps(repo):
    """One `unittest` step per directory under tools/ and acceptance/ that holds test_*.py files."""
    steps = []
    for directory in sorted({path.parent for path in [*repo.glob("tools/*/test_*.py"), *repo.glob("acceptance/test_*.py")]}):
        modules = sorted(path.stem for path in directory.glob("test_*.py"))
        steps.append(Step(f"tests {directory.relative_to(repo).as_posix()}", directory, ["python", "-m", "unittest", "-q", *modules]))
    return steps


def deny_steps(which):
    return [_cargo("cargo deny", "deny", "--locked", "check")] if which("cargo-deny") else []


def _wsl_path(path):
    text = path.as_posix()
    return f"/mnt/{text[0].lower()}{text[2:]}" if len(text) > 1 and text[1] == ":" else text


def wsl_steps(repo):
    script = f"cd {_wsl_path(repo)} && CARGO_TARGET_DIR={WSL_TARGET} /root/.cargo/bin/cargo test --workspace --locked"
    return [Step("wsl cargo test", Path("."), ["wsl", "-d", "Ubuntu", "-u", "root", "--", "bash", "-c", script])]


def build_plan(repo, base, wsl, which=shutil.which):
    steps = [*rust_steps(), *gate_steps(base), *python_test_steps(repo), *deny_steps(which)]
    return steps + wsl_steps(repo) if wsl else steps


def run_plan(steps, execute, print_line=print):
    results = []
    for step in steps:
        print_line(f"== {step.name}")
        code = execute(step)
        results.append(Result(step.name, "pass" if code == 0 else "FAIL"))
        print_line(f"-- {step.name}: {results[-1].status}")
    print_line("")
    for result in results:
        print_line(f"{result.status:4} {result.name}")
    print_line(f"not run locally: {NOT_RUN}")
    return results


def exit_code(results):
    return 1 if any(result.status == "FAIL" for result in results) else 0


def resolve(argv, search_path):
    """argv with the program replaced by its full path found on `search_path`; Windows would use the parent's PATH."""
    found = shutil.which(argv[0], path=search_path)
    return [found, *argv[1:]] if found else list(argv)


def _execute_in(repo):
    path = str(Path.home() / ".cargo" / "bin") + os.pathsep + os.environ.get("PATH", "")
    env = {**os.environ, "PATH": path}

    def execute(step):
        try:
            return subprocess.run(resolve(step.argv, path), cwd=repo / step.cwd, env=env, check=False).returncode
        except FileNotFoundError:
            return MISSING

    return execute


def main(argv, print_line=print):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--repo", type=Path, default=Path("."))
    parser.add_argument("--base", default="origin/main", help="base of the diff hash (origin/develop for an ordinary card)")
    parser.add_argument("--wsl", action="store_true", help="also run the tests on Linux through WSL")
    parser.add_argument("--list", action="store_true", help="print the plan and stop")
    args = parser.parse_args(argv)
    repo = args.repo.resolve()
    steps = build_plan(repo, args.base, args.wsl)
    if args.list:
        for step in steps:
            print_line(f"{step.name}: {' '.join(step.argv)}")
        return 0
    return exit_code(run_plan(steps, _execute_in(repo), print_line))


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
