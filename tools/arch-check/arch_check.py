"""Check the layer and dependency rules of the engine workspace (card 0c-2).

Rules: wiki/architecture/overview.md section 2 and ports-and-adapters.md section 2;
the numbers and names are in gates/architecture.toml (a protected path). The crate
graph comes from `cargo metadata --no-deps`; source files of layers 0-2 are scanned
for banned uses.

Findings: layer (upward edge; only layer 4 may use layer 3), same-layer (an
undeclared edge between two crates of layers 1-3), forbidden-edge, cycle, unmapped
(a workspace crate with no layer), external and banned-crate (non-std crates in
layers 0-2), banned-use (source patterns in layers 0-2).

Known gaps (reviews/0c-2.md): the source scan is textual. A banned name built by a
macro, `std` reached through a type alias or a re-export from another crate, or a
file pulled in by a macro other than `include!` escapes it (`#[path]`, also inside `cfg_attr`, and `include!`
are refused outright in layers 0-2, as are extern blocks, `#[link]`, and `arch`/`asm!` outside
`base`; a target whose source file is outside the crate folder is the finding `src-outside`).
A `path =` key of any other attribute, such as `#[cfg(path = "x")]`, is refused too, and `]` inside
an attribute ends the attribute match early. A rule with `allow_in_tests = true` (card 1-4) skips `#[cfg(test)]`
items, a `#![cfg(test)]` that is an inner attribute of the file itself (not one inside a module or a macro body) and the crate's `tests/` folder (except a file that a non-test cargo target builds, e.g. a `[lib] path`); the exemption is by character position, so production code on the same line as a test item is still checked; the match is textual and exact (`cfg(not(test))` and
`cfg(any(test, ..))` are not tests; an out-of-line `#[cfg(test)] mod x;` file is still scanned; the exempt part ends at the item's closing brace or at the first `;` or `,` outside brackets, so a `#[cfg(test)]` item with a comma in generics such as `fn f<A, B>()` is cut short, which is the safe direction). Dev-dependencies are exempt from the layer,
allowlist and cycle rules (a test may use a null adapter) but not from the
banned-crate list and the forbidden edges. Features and target-specific
dependencies are treated as normal ones.

    arch_check.py --config gates/architecture.toml [--repo .] [--metadata metadata.json]

Run it from a trusted checkout of main: the tool is not taken from the PR tree.
"""

import argparse
import fnmatch
import json
import os
import re
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

LAYER_COUNT = 5
ENGINE_LAYERS = (0, 1, 2)
SAME_LAYER_CHECKED = (1, 2, 3)
ADAPTER_LAYER, APPLICATION_LAYER = 3, 4
GIT_ENV = {**os.environ}


class ArchError(Exception):
    pass


@dataclass(frozen=True)
class Finding:
    rule: str
    message: str


@dataclass(frozen=True)
class UseRule:
    pattern: re.Pattern
    reason: str
    crates: tuple
    allow_in: tuple
    allow_in_tests: bool = False


@dataclass(frozen=True)
class Config:
    layers: dict
    exempt: tuple
    glue: tuple
    allowed_edges: tuple
    forbidden_edges: tuple
    allowed_external: tuple
    banned_crates: tuple
    banned_use: tuple


def _pairs(rules, key):
    pairs = [tuple(pair) for pair in rules.get(key, [])]
    if any(len(pair) != 2 for pair in pairs):
        raise ValueError(f"every entry of '{key}' must be a pair of crate names")
    return tuple(pairs)


def _use_rule(entry):
    if not entry.get("pattern") or not entry.get("reason"):
        raise ValueError("a [[banned_use]] entry needs a pattern and a reason")
    try:
        pattern = re.compile(entry["pattern"])
    except re.error as error:
        raise ValueError(f"bad regex '{entry['pattern']}': {error}") from error
    in_tests = entry.get("allow_in_tests", False)
    if not isinstance(in_tests, bool):
        raise ValueError("allow_in_tests must be true or false")
    return UseRule(pattern, entry["reason"], tuple(entry.get("crates", ())), tuple(entry.get("allow_in", ())), in_tests)


def load_config(text):
    data = tomllib.loads(text)
    layers, rules = data.get("layers", {}), data.get("rules", {})
    if not layers:
        raise ValueError("the [layers] table is empty")
    for name, layer in layers.items():
        if not isinstance(layer, int) or not 0 <= layer < LAYER_COUNT:
            raise ValueError(f"layer of '{name}' must be an integer 0-{LAYER_COUNT - 1}")
    return Config(
        layers=dict(layers), exempt=tuple(rules.get("exempt", ())), glue=tuple(rules.get("glue", ())),
        allowed_edges=_pairs(rules, "allowed_edges"), forbidden_edges=_pairs(rules, "forbidden_edges"),
        allowed_external=tuple(rules.get("allowed_external", ())), banned_crates=tuple(rules.get("banned_crates", ())),
        banned_use=tuple(_use_rule(entry) for entry in data.get("banned_use", [])),
    )


def _matches(name, patterns):
    return any(fnmatch.fnmatchcase(name, pattern) for pattern in patterns)


def _in_pairs(source, target, pairs):
    return any(fnmatch.fnmatchcase(source, a) and fnmatch.fnmatchcase(target, b) for a, b in pairs)


def layer_of(name, config):
    """Exact entries win; among overlapping globs the lowest layer (the strictest) wins; None if unmapped."""
    if name in config.layers:
        return config.layers[name]
    found = [layer for pattern, layer in config.layers.items() if fnmatch.fnmatchcase(name, pattern)]
    return min(found) if found else None


def _edge_findings(source, target, kind, layers, config):
    """Findings for one workspace dependency edge."""
    ls, lt = layers[source], layers[target]
    found = []
    if kind == "dev":
        return found
    if lt > ls:
        found.append(Finding("layer", f"{source} (L{ls}) depends on {target} (L{lt}), a higher layer"))
    elif lt == ls and ls in SAME_LAYER_CHECKED and not _matches(source, config.glue) and not _in_pairs(source, target, config.allowed_edges):
        found.append(Finding("same-layer", f"{source} depends on {target} (both L{ls}) and the edge is not declared"))
    return found


def _external_findings(crate, layer, dependency, config):
    name, kind = dependency["name"], dependency.get("kind")
    if layer not in ENGINE_LAYERS:
        return []
    if name in config.banned_crates:
        return [Finding("banned-crate", f"{crate} (L{layer}) uses the banned crate {name}")]
    if kind != "dev" and name not in config.allowed_external:
        return [Finding("external", f"{crate} (L{layer}) uses the external crate {name}, which is not allowlisted")]
    return []


def _cycles(graph):
    """One finding per dependency cycle among the given {crate: [crate]} edges."""
    state, found, stack = {}, {}, []

    def visit(node):
        state[node] = "open"
        stack.append(node)
        for nxt in graph.get(node, ()):
            if state.get(nxt) == "open":
                loop = stack[stack.index(nxt):]
                found[frozenset(loop)] = " -> ".join(loop + [nxt])
            elif nxt not in state:
                visit(nxt)
        stack.pop()
        state[node] = "done"

    for node in sorted(graph):
        if node not in state:
            visit(node)
    return [Finding("cycle", f"dependency cycle {path}") for path in found.values()]


def _forbidden_findings(graph, config):
    """Forbidden edges, direct or through other crates (dev dependencies count here)."""
    found = []
    for start in sorted(graph):
        seen, queue = {start: [start]}, [start]
        while queue:
            node = queue.pop(0)
            for nxt in graph.get(node, ()):
                if nxt in seen:
                    continue
                seen[nxt] = seen[node] + [nxt]
                queue.append(nxt)
                if _in_pairs(start, nxt, config.forbidden_edges):
                    found.append(Finding("forbidden-edge", f"{start} must not depend on {nxt} ({' -> '.join(seen[nxt])})"))
    return found


_TOKEN = re.compile(r"//[^\n]*|/\*|(?<!\w)(?:br|cr|r)(#*)\"|\"|'(?:\\[^']*|[^\\'\n])'")


def _block_end(text, pos):
    depth = 1
    while depth and pos < len(text):
        pair = text[pos:pos + 2]
        depth += (pair == "/*") - (pair == "*/")
        pos += 2 if pair in ("/*", "*/") else 1
    return pos


def _string_end(text, pos):
    while pos < len(text):
        if text[pos] == "\\":
            pos += 2
        elif text[pos] == '"':
            return pos + 1
        else:
            pos += 1
    return len(text)


def _token_end(text, match):
    token = match.group(0)
    if token == "/*":
        return _block_end(text, match.end())
    if token == '"':
        return _string_end(text, match.end())
    if token.lstrip("bc").startswith("r"):
        closer = '"' + match.group(1)
        index = text.find(closer, match.end())
        return len(text) if index < 0 else index + len(closer)
    return match.end()


def strip_noncode(text):
    """The text with comments and string or char literals blanked; line numbers are kept."""
    out, pos = [], 0
    while match := _TOKEN.search(text, pos):
        end = _token_end(text, match)
        out.append(text[pos:match.start()])
        out.append(re.sub(r"[^\n]", " ", text[match.start():end]))
        pos = end
    out.append(text[pos:])
    return "".join(out)


def _crate_folder(package):
    root = Path(package["manifest_path"]).parent
    if not root.is_dir():
        raise ArchError(f"{package['name']}: crate folder {root} does not exist")
    return root


def _source_files(package):
    """Every .rs file of the crate (src, tests, benches, examples, build.rs), not those of nested crates."""
    root = _crate_folder(package)
    files = []
    for path in sorted(root.rglob("*.rs")):
        relative = path.relative_to(root)
        nested = any((root / folder / "Cargo.toml").exists() for folder in relative.parents if str(folder) != ".")
        if relative.parts[0] != "target" and not nested:  # only the crate's own build output, not a module called target
            files.append(path)
    return files


def _source_roots_findings(package):
    """A cargo target whose source file is outside the crate folder hides code from the scan."""
    root = _crate_folder(package).resolve()
    paths = {target.get("src_path") or "" for target in package.get("targets", [])}
    outside = sorted(path for path in paths if not Path(os.path.normpath(path)).resolve().is_relative_to(root))
    return [Finding("src-outside", f"{package['name']}: target source {path} is outside the crate folder, so the source scan would not see it") for path in outside]


def _split_top(text):
    parts, depth, start = [], 0, 0
    for index, char in enumerate(text):
        depth += (char == "{") - (char == "}")
        if char == "," and depth == 0:
            parts.append(text[start:index])
            start = index + 1
    return [part for part in parts + [text[start:]] if part.strip()]


def _join(prefix, leaf):
    leaf = re.sub(r"\s+", "", leaf)
    if leaf == "self":
        return prefix
    return f"{prefix}::{leaf}" if prefix else leaf


def flatten_use(tree, prefix=""):
    """The paths of a `use` tree, braces expanded; an alias also gives `path as alias`."""
    brace = tree.find("{")
    if brace < 0:
        leaf, *alias = re.split(r"\s+as\s+", tree.strip(), maxsplit=1)
        path = _join(prefix, leaf)
        return [path, f"{path} as {alias[0].strip()}"] if alias else [path]
    head = re.sub(r"\s+", "", tree[:brace]).rstrip(":")
    base = _join(prefix, head) if head else prefix
    found = []
    for part in _split_top(tree[brace + 1:tree.rindex("}")]):
        found += flatten_use(part, base)
    return found


_USE = re.compile(r"(?<![\w])use\s+([^;]*);")


def _virtual_uses(text):
    """(position, 'use path;') for every path of every use statement, so grouped imports are seen as single paths."""
    return [(match.start(), f"use {path};") for match in _USE.finditer(text) for path in flatten_use(match.group(1))]


def _rule_hits(rule, text, virtual):
    """Character positions where the rule matches, plain or as a flattened use path."""
    return {match.start() for match in rule.pattern.finditer(text)} | {pos for pos, use in virtual if rule.pattern.search(use)}


_CFG_TEST = re.compile(r"#\s*(!?)\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]")
_INNER_ATTRIBUTES = re.compile(r"(?:\s|#\s*!\s*\[[^\]]*\])*")


def _item_end(text, pos):
    """The end of what a `#[cfg(test)]` at pos applies to: the item's closing brace, the first `;` or `,` outside
    brackets, or the bracket that closes the enclosing block (a field, argument or match arm). Never later."""
    depth = 0
    for index in range(pos, len(text)):
        char = text[index]
        depth += (char in "([{") - (char in ")]}")
        if depth < 0 or (depth == 0 and char in ",;}"):
            return index
    return len(text)


def _test_spans(text):
    """(start, end) character spans of test-only code: `#[cfg(test)]` items, or the whole file for a `#![cfg(test)]`
    that is an inner attribute of the file itself (only whitespace and other inner attributes before it). Any other
    cfg form, and an inner attribute inside a module or a macro body, is not a test."""
    spans = []
    for match in _CFG_TEST.finditer(text):
        if not match.group(1):
            spans.append((match.start(), _item_end(text, match.end()) + 1))
        elif _INNER_ATTRIBUTES.fullmatch(text[:match.start()]):
            return [(0, len(text))]
    return spans


def _built_sources(package):
    """Resolved source files of every cargo target that is not a test target: they are production code wherever they sit."""
    return {Path(os.path.normpath(target["src_path"])).resolve() for target in package.get("targets", []) if target.get("kind") != ["test"]}


def _is_test_file(path, root, built):
    return path.relative_to(root).parts[0] == "tests" and path.resolve() not in built


def _use_findings(package, rules):
    crate = package["name"]
    active = [rule for rule in rules if (not rule.crates or _matches(crate, rule.crates)) and not _matches(crate, rule.allow_in)]
    found, root, built = set(), _crate_folder(package), _built_sources(package)
    for path in _source_files(package) if active else []:
        text = strip_noncode(path.read_text(encoding="utf-8", errors="replace"))
        virtual, spans, test_file = _virtual_uses(text), _test_spans(text), _is_test_file(path, root, built)
        for rule in active:
            hits = _rule_hits(rule, text, virtual)
            if rule.allow_in_tests:
                hits = set() if test_file else {pos for pos in hits if not any(start <= pos < end for start, end in spans)}
            found |= {Finding("banned-use", f"{crate} {path.name}:{text.count(chr(10), 0, pos) + 1}: {rule.reason}") for pos in hits}
    return sorted(found, key=lambda finding: finding.message)


def _package_findings(package, layers, config, names):
    crate, layer = package["name"], layers[package["name"]]
    found = []
    for dependency in package["dependencies"]:
        name = dependency["name"]
        if name in layers and dependency.get("source") is None:
            found += _edge_findings(crate, name, dependency.get("kind"), layers, config)
        elif name in names and dependency.get("source") is None:
            found.append(Finding("exempt-dependency", f"{crate} depends on {name}, which is exempt from the architecture rules"))
        else:
            found += _external_findings(crate, layer, dependency, config)
    if layer in ENGINE_LAYERS:
        found += _source_roots_findings(package) + _use_findings(package, config.banned_use)
    return found


def check(metadata, config):
    """Findings for a `cargo metadata --no-deps` document, sorted."""
    packages = [p for p in metadata.get("packages", []) if not _matches(p["name"], config.exempt)]
    names = {p["name"] for p in metadata.get("packages", [])}
    layers, found = {}, []
    for package in packages:
        layer = layer_of(package["name"], config)
        if layer is None:
            found.append(Finding("unmapped", f"{package['name']} has no layer in the architecture rules"))
        else:
            layers[package["name"]] = layer
    for package in packages:
        if package["name"] in layers:
            found += _package_findings(package, layers, config, names)
    found += _graph_findings(packages, layers, config)
    found += _unlisted_crates(metadata)
    return sorted(set(found), key=lambda finding: (finding.rule, finding.message))


def _workspace_edges(packages, layers, with_dev):
    return {
        p["name"]: [d["name"] for d in p["dependencies"] if d["name"] in layers and d.get("source") is None and (with_dev or d.get("kind") != "dev")]
        for p in packages if p["name"] in layers
    }


def _graph_findings(packages, layers, config):
    return _cycles(_workspace_edges(packages, layers, with_dev=False)) + _forbidden_findings(_workspace_edges(packages, layers, with_dev=True), config)


SKIPPED_FOLDERS = {"target", ".git", "node_modules", "mutants.out"}


def _unlisted_crates(metadata):
    """Cargo.toml files in the workspace folder that belong to no package (a crate the checks cannot see)."""
    root = metadata.get("workspace_root")
    if not root:
        return []
    listed = {os.path.normcase(os.path.realpath(Path(p["manifest_path"]).parent)) for p in metadata.get("packages", [])}
    found = []
    for folder, children, files in os.walk(root):
        children[:] = [child for child in children if child not in SKIPPED_FOLDERS]
        here = os.path.normcase(os.path.realpath(folder))
        if "Cargo.toml" in files and here not in listed and Path(folder) != Path(root):
            found.append(Finding("unlisted-crate", f"{folder} has a Cargo.toml but is not a workspace member"))
    return found


def _load_metadata(repo, metadata_file):
    if metadata_file:
        return json.loads(metadata_file.read_text(encoding="utf-8"))
    try:
        done = subprocess.run(["cargo", "metadata", "--format-version", "1", "--no-deps"], cwd=repo, env=GIT_ENV, capture_output=True, check=False)
    except OSError as error:
        raise ArchError(f"cannot run cargo: {error}") from error
    if done.returncode != 0:
        raise ArchError(f"cargo metadata failed: {done.stderr.decode(errors='replace').strip()}")
    return json.loads(done.stdout)


def main(argv, print_line=print):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--repo", type=Path, default=Path("."))
    parser.add_argument("--metadata", type=Path, help="a saved `cargo metadata --no-deps` document instead of running cargo")
    args = parser.parse_args(argv)
    try:
        config = load_config(args.config.read_text(encoding="utf-8"))
        metadata = _load_metadata(args.repo, args.metadata)
        if not metadata.get("packages"):
            raise ArchError("the metadata lists no packages; is this a Cargo workspace?")
        found = check(metadata, config)
    except (ArchError, ValueError, OSError, KeyError) as error:
        print_line(f"FAIL {error}")
        return 1
    for finding in found:
        print_line(f"VIOLATION {finding.rule} {finding.message}")
    print_line(f"{'FAIL' if found else 'OK'} {len(metadata['packages'])} crates checked, {len(found)} violations")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
