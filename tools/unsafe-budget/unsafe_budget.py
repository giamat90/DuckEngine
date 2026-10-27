"""Check the unsafe budget of the engine crates (gate G3, card 1-31).

Rules (ADR-0001 section 6, rules-digest section 4): in the crates of layers 0-2, `unsafe` code is at
most `max_percent` of the code lines, every file that holds `unsafe` is on the allowlist in
gates/unsafe-budget.toml with a line budget, and every `unsafe` block or impl has a `// SAFETY:`
comment directly above it (an `unsafe fn` needs a `# Safety` doc section or a SAFETY comment).

Findings: unsafe-outside-allowlist, missing-safety, module-budget, unsafe-ratio, stale-allowlist (an
allowlisted file that does not exist), unmapped (a workspace crate with no layer).

The count is textual (comments and strings are blanked first): an `unsafe` span runs from the keyword
to the closing brace of its block, or to the end of the line when a `;` comes first.

    unsafe_budget.py --config gates/unsafe-budget.toml --architecture gates/architecture.toml [--repo .]
                     [--metadata metadata.json]

Run it from a trusted checkout of main, like the other gate tools.
"""

import argparse
import re
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "arch-check"))
import arch_check  # noqa: E402  (needs the path set above)

CHECKED_LAYERS = (0, 1, 2)
_UNSAFE = re.compile(r"\bunsafe\b")
_AFTER_UNSAFE = re.compile(r"\s*(\w+)?")
_SAFETY_COMMENT = re.compile(r"SAFETY:")
_SAFETY_SECTION = re.compile(r"#\s*Safety|SAFETY:")
_OPEN, _CLOSE = "([", ")]"


@dataclass(frozen=True)
class Budget:
    max_percent: float
    modules: dict


@dataclass(frozen=True)
class Report:
    findings: list
    unsafe_lines: int
    code_lines: int


def load_budget(text):
    data = tomllib.loads(text)
    percent = data.get("max_percent")
    if isinstance(percent, bool) or not isinstance(percent, (int, float)):
        raise ValueError("max_percent must be a number")
    if not 0 <= percent <= 100:  # also false for nan
        raise ValueError("max_percent must be between 0 and 100")
    modules = {}
    for entry in data.get("modules", []):
        path, lines = entry.get("path"), entry.get("max_unsafe_lines")
        if not isinstance(path, str) or isinstance(lines, bool) or not isinstance(lines, int):
            raise ValueError("a [[modules]] entry needs a path and an integer max_unsafe_lines")
        if path in modules:
            raise ValueError(f"{path} is listed twice")
        modules[path] = lines
    return Budget(float(percent), modules)


def load_architecture(text):
    return arch_check.load_config(text)


def _span_end(code, start):
    """Index just past the block that follows `unsafe` at `start`, or past the `;` that ends a declaration."""
    depth = 0
    pos = start
    while pos < len(code):
        char = code[pos]
        if char in _OPEN:
            depth += 1
        elif char in _CLOSE:
            depth -= 1
        elif char == ";" and depth <= 0:
            return pos + 1
        elif char == "{" and depth <= 0:
            return _matching_brace(code, pos)
        pos += 1
    return len(code)


def _matching_brace(code, open_pos):
    depth = 0
    for pos in range(open_pos, len(code)):
        if code[pos] == "{":
            depth += 1
        elif code[pos] == "}":
            depth -= 1
            if depth == 0:
                return pos + 1
    return len(code)


def _line_of(code, index):
    return code.count("\n", 0, index)


def _comment_run_above(lines, index):
    """The text of the contiguous comment and attribute lines directly above line `index`."""
    run = []
    while index > 0:
        index -= 1
        stripped = lines[index].strip()
        if not stripped.startswith(("//", "#[", "#![")) and not stripped.startswith("*"):
            break
        run.append(stripped)
    return "\n".join(run)


def _has_safety(kind, lines, index):
    if _SAFETY_COMMENT.search(lines[index]):
        return True
    above = _comment_run_above(lines, index)
    return bool(_SAFETY_SECTION.search(above) if kind == "fn" else _SAFETY_COMMENT.search(above))


def _kind(code, token_end):
    word = _AFTER_UNSAFE.match(code, token_end).group(1)
    if word in ("fn", "extern"):
        return "fn"
    return "impl" if word == "impl" else "other"


def scan_file(text):
    """(unsafe line numbers as a set, code line count, line numbers lacking a safety note)."""
    code = arch_check.strip_noncode(text)
    raw = text.split("\n")
    code_rows = code.split("\n")
    unsafe, missing, impl_spans = set(), [], []
    for match in _UNSAFE.finditer(code):
        first = _line_of(code, match.start())
        last = _line_of(code, _span_end(code, match.end()) - 1)
        kind = _kind(code, match.end())
        unsafe.update(row for row in range(first, last + 1) if code_rows[row].strip())
        if kind == "impl":
            impl_spans.append((first, last))
        inside_impl = kind == "fn" and any(a < first <= b for a, b in impl_spans)
        if not inside_impl and not _has_safety(kind, raw, first):
            missing.append(first + 1)
    return unsafe, sum(1 for row in code_rows if row.strip()), missing


def _relative(path, root):
    return path.resolve().relative_to(root.resolve()).as_posix()


def _file_findings(relative, unsafe, missing, budget):
    if not unsafe:
        return []
    if relative not in budget.modules:
        return [arch_check.Finding("unsafe-outside-allowlist", f"{relative}: {len(unsafe)} unsafe lines in a file that is not on the allowlist")]
    found = [arch_check.Finding("missing-safety", f"{relative}:{row}: unsafe without a SAFETY note") for row in missing]
    if len(unsafe) > budget.modules[relative]:
        found.append(arch_check.Finding("module-budget", f"{relative}: {len(unsafe)} unsafe lines, budget {budget.modules[relative]}"))
    return found


def _checked_packages(metadata, architecture):
    packages, findings = [], []
    for package in metadata["packages"]:
        name = package["name"]
        layer = arch_check.layer_of(name, architecture)
        if arch_check._matches(name, architecture.exempt):
            continue
        if layer is None:
            findings.append(arch_check.Finding("unmapped", f"crate '{name}' has no layer in the architecture config"))
        elif layer in CHECKED_LAYERS:
            packages.append(package)
    return packages, findings


def check(metadata, budget, architecture):
    root = Path(metadata["workspace_root"])
    packages, findings = _checked_packages(metadata, architecture)
    unsafe_total = code_total = 0
    for package in packages:
        for path in arch_check._source_files(package):
            unsafe, code_lines, missing = scan_file(path.read_text(encoding="utf-8"))
            unsafe_total, code_total = unsafe_total + len(unsafe), code_total + code_lines
            findings += _file_findings(_relative(path, root), unsafe, missing, budget)
    findings += [arch_check.Finding("stale-allowlist", f"{relative}: allowlisted file does not exist") for relative in budget.modules if not (root / relative).is_file()]
    if code_total and unsafe_total * 100 > budget.max_percent * code_total:
        findings.append(arch_check.Finding("unsafe-ratio", f"{unsafe_total} of {code_total} code lines are unsafe, limit {budget.max_percent}%"))
    return Report(findings, unsafe_total, code_total)


def main(argv, print_line=print):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--architecture", type=Path, required=True)
    parser.add_argument("--repo", type=Path, default=Path("."))
    parser.add_argument("--metadata", type=Path, help="a saved `cargo metadata --no-deps` document instead of running cargo")
    args = parser.parse_args(argv)
    try:
        budget = load_budget(args.config.read_text(encoding="utf-8"))
        architecture = load_architecture(args.architecture.read_text(encoding="utf-8"))
        metadata = arch_check._load_metadata(args.repo, args.metadata)
        report = check(metadata, budget, architecture)
    except (arch_check.ArchError, ValueError, OSError, KeyError) as error:
        print_line(f"FAIL {error}")
        return 1
    for finding in report.findings:
        print_line(f"VIOLATION {finding.rule} {finding.message}")
    percent = 100 * report.unsafe_lines / report.code_lines if report.code_lines else 0
    print_line(f"{'FAIL' if report.findings else 'OK'} {report.unsafe_lines} unsafe of {report.code_lines} code lines ({percent:.2f}%), {len(report.findings)} violations")
    return 1 if report.findings else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
