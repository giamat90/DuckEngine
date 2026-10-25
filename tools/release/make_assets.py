"""Build the release assets for one version (card 0b-6).

Writes, into --out: the source archive, a dummy artifact, a metric report, a
baselines record and SHA256SUMS. The dummy artifact, the metric report and the
baselines are placeholders until the real packaging and metrics exist (phase 0b
proves the pipeline, not the content). Nothing is written for an invalid version.
"""

import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

SEMVER = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)")
SUMS = "SHA256SUMS"


def _git(repo, *args):
    return subprocess.run(["git", *args], cwd=repo, capture_output=True, check=True).stdout


def build(repo, version, out, extra=()):
    """Write all assets into `out` and return their paths (SHA256SUMS last).

    Files in `extra` are copied in and checksummed too.
    """
    if not SEMVER.fullmatch(version):
        raise ValueError(f"not a plain X.Y.Z version: {version!r}")
    commit = _git(repo, "rev-parse", "HEAD").decode().strip()
    out.mkdir(parents=True, exist_ok=True)

    prefix = f"duckengine-v{version}"
    archive = out / f"{prefix}-source.tar.gz"
    archive.write_bytes(_git(repo, "archive", "--format=tar.gz", f"--prefix={prefix}/", "HEAD"))

    dummy = out / "dummy-artifact.txt"
    dummy.write_text(f"DuckEngine {version} placeholder artifact.\nCommit {commit}\n", encoding="utf-8", newline="\n")
    report = out / "metric-report.md"
    report.write_text(
        f"# Metric report {version}\n\nPlaceholder: the metric report is generated in a later phase.\n\nCommit {commit}\n",
        encoding="utf-8",
        newline="\n",
    )
    baselines = out / "baselines.json"
    baselines.write_text(
        json.dumps({"version": version, "commit": commit, "metrics": {}}, indent=2) + "\n", encoding="utf-8", newline="\n"
    )

    files = [archive, dummy, report, baselines]
    for source in extra:
        copy = out / Path(source).name
        copy.write_bytes(Path(source).read_bytes())
        files.append(copy)
    sums = out / SUMS
    sums.write_text(
        "".join(f"{hashlib.sha256(f.read_bytes()).hexdigest()}  {f.name}\n" for f in files), encoding="utf-8", newline="\n"
    )
    return [*files, sums]


def main(argv, print_line=print):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--repo", type=Path, default=Path("."))
    parser.add_argument("--version", required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--extra", type=Path, nargs="*", default=[], help="more files to include and checksum")
    args = parser.parse_args(argv)
    try:
        files = build(args.repo, args.version, args.out, args.extra)
    except subprocess.CalledProcessError as error:
        print_line(f"error: git failed: {error.stderr.decode(errors='replace').strip()}")
        return 1
    except (ValueError, OSError) as error:
        print_line(f"error: {error}")
        return 1
    for f in files:
        print_line(f.name)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
