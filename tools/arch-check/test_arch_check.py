import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

import arch_check as ac

REPO = Path(__file__).resolve().parents[2]

CONFIG = """
[layers]
base = 0
platform = 1
jobs = 1
ecs = 1
physics = 2
render = 2
frame = 2
"adapter_*" = 3
"app_*" = 4

[rules]
exempt = ["skeleton"]
glue = ["frame"]
allowed_edges = [["jobs", "platform"], ["ecs", "jobs"], ["render", "jobs"]]
forbidden_edges = [["physics", "ecs"]]
allowed_external = ["smallvec"]
banned_crates = ["libc"]

[[banned_use]]
pattern = '\\bstd::(?:fs|io)\\b'
reason = "OS-facing std module"
allow_in = ["platform"]

[[banned_use]]
pattern = '\\.(?:sin|cos)\\('
reason = "libm float function"
crates = ["physics"]
"""


def dep(name, kind=None, external=False):
    return {"name": name, "kind": kind, "source": "registry+https://x" if external else None}


class Workspace:
    """A throwaway workspace: cargo-metadata-shaped dict plus real src folders."""

    def __init__(self, test, crates):
        self.dir = Path(tempfile.mkdtemp())
        test.addCleanup(lambda: __import__("shutil").rmtree(self.dir, ignore_errors=True))
        self.packages = []
        for name, (deps, sources) in crates.items():
            src = self.dir / name / "src"
            src.mkdir(parents=True)
            for file, text in sources.items():
                (src / file).write_text(text, encoding="utf-8")
            self.packages.append({"name": name, "manifest_path": str(self.dir / name / "Cargo.toml"), "dependencies": deps})
        self.metadata = {"packages": self.packages, "workspace_members": []}

    def check(self, config=CONFIG):
        return ac.check(self.metadata, ac.load_config(config))


def make(test, crates, config=CONFIG):
    return Workspace(test, crates).check(config)


def rules(findings):
    return sorted({finding.rule for finding in findings})


class Layers(unittest.TestCase):
    def test_a_clean_workspace_has_no_findings(self):
        found = make(self, {"base": ([], {"lib.rs": "pub fn a() {}"}), "jobs": ([dep("base"), dep("platform")], {}),
                            "platform": ([dep("base")], {}), "app_game": ([dep("render"), dep("adapter_gl")], {}),
                            "render": ([dep("base"), dep("jobs")], {}), "adapter_gl": ([dep("render")], {})})
        self.assertEqual([], found)

    def test_a_lower_layer_depending_on_a_higher_one_is_a_violation(self):
        found = make(self, {"base": ([dep("jobs")], {}), "jobs": ([], {})})
        self.assertEqual(["layer"], rules(found))
        self.assertIn("base", found[0].message)

    def test_runtime_depending_on_a_domain_module_is_a_violation(self):
        self.assertEqual(["layer"], rules(make(self, {"jobs": ([dep("render")], {}), "render": ([], {})})))

    def test_domain_or_runtime_depending_on_an_adapter_is_a_violation(self):
        found = make(self, {"render": ([dep("adapter_gl")], {}), "adapter_gl": ([], {}), "jobs": ([dep("adapter_gl")], {})})
        self.assertEqual(["layer", "layer"], [finding.rule for finding in found])

    def test_an_adapter_depending_on_an_application_is_a_violation(self):
        self.assertEqual(["layer"], rules(make(self, {"adapter_gl": ([dep("app_game")], {}), "app_game": ([], {})})))

    def test_a_same_layer_edge_must_be_declared(self):
        found = make(self, {"physics": ([dep("render")], {}), "render": ([], {})})
        self.assertEqual(["same-layer"], rules(found))

    def test_a_declared_same_layer_edge_passes(self):
        self.assertEqual([], make(self, {"ecs": ([dep("jobs")], {}), "jobs": ([], {})}))

    def test_a_forbidden_edge_is_a_violation_even_towards_a_lower_layer(self):
        found = make(self, {"physics": ([dep("ecs")], {}), "ecs": ([], {})})
        self.assertEqual(["forbidden-edge"], rules(found))

    def test_glue_may_depend_on_several_domain_modules(self):
        self.assertEqual([], make(self, {"frame": ([dep("physics"), dep("render")], {}), "physics": ([], {}), "render": ([], {})}))

    def test_two_adapters_may_not_depend_on_each_other(self):
        self.assertEqual(["same-layer"], rules(make(self, {"adapter_a": ([dep("adapter_b")], {}), "adapter_b": ([], {})})))

    def test_a_dev_dependency_on_a_higher_layer_is_allowed(self):
        self.assertEqual([], make(self, {"render": ([dep("adapter_null", "dev")], {}), "adapter_null": ([], {})}))

    def test_a_crate_that_is_not_mapped_is_a_violation(self):
        found = make(self, {"mystery": ([], {})})
        self.assertEqual(["unmapped"], rules(found))

    def test_an_exempt_crate_is_not_checked(self):
        self.assertEqual([], make(self, {"skeleton": ([dep("app_game"), dep("libc", external=True)], {"lib.rs": "use std::fs;"}), "app_game": ([], {})}))

    def test_a_glob_maps_crates_by_prefix(self):
        self.assertEqual(["layer"], rules(make(self, {"base": ([dep("app_x")], {}), "app_x": ([], {})})))

    def test_a_dependency_cycle_is_a_violation(self):
        found = make(self, {"ecs": ([dep("jobs")], {}), "jobs": ([dep("ecs")], {})})
        self.assertIn("cycle", rules(found))


class External(unittest.TestCase):
    def test_an_external_crate_in_layers_0_to_2_must_be_allowlisted(self):
        self.assertEqual(["external"], rules(make(self, {"base": ([dep("serde", external=True)], {})})))

    def test_an_allowlisted_external_crate_passes(self):
        self.assertEqual([], make(self, {"base": ([dep("smallvec", external=True)], {})}))

    def test_adapters_and_applications_may_use_external_crates(self):
        self.assertEqual([], make(self, {"adapter_gl": ([dep("glfw", external=True)], {}), "app_x": ([dep("serde", external=True)], {})}))

    def test_a_banned_crate_is_refused_in_layers_0_to_2_in_every_dependency_kind(self):
        found = make(self, {"base": ([dep("libc", "dev", external=True)], {}), "jobs": ([dep("libc", "build", external=True)], {})})
        self.assertEqual(["banned-crate", "banned-crate"], [finding.rule for finding in found])

    def test_an_external_dev_dependency_is_allowed(self):
        self.assertEqual([], make(self, {"base": ([dep("proptest", "dev", external=True)], {})}))


class BannedUse(unittest.TestCase):
    def test_a_banned_use_in_a_runtime_crate_is_a_violation(self):
        found = make(self, {"jobs": ([], {"lib.rs": "use std::fs::File;\n"})})
        self.assertEqual(["banned-use"], rules(found))
        self.assertIn("lib.rs:1", found[0].message)

    def test_the_same_use_in_an_adapter_or_an_allowed_crate_is_fine(self):
        self.assertEqual([], make(self, {"adapter_gl": ([], {"lib.rs": "use std::fs;"}), "platform": ([], {"lib.rs": "use std::io;"})}))

    def test_comments_and_strings_do_not_count(self):
        text = '// use std::fs;\n/* std::io\n still comment */\nlet s = "std::fs"; let c = \'a\';\n'
        self.assertEqual([], make(self, {"jobs": ([], {"lib.rs": text})}))

    def test_code_after_a_block_comment_still_counts(self):
        self.assertEqual(["banned-use"], rules(make(self, {"jobs": ([], {"lib.rs": "/* x */ use std::fs;"})})))

    def test_a_rule_limited_to_a_crate_applies_only_there(self):
        found = make(self, {"physics": ([], {"lib.rs": "let y = x.sin();"}), "render": ([], {"lib.rs": "let y = x.sin();"})})
        self.assertEqual(1, len(found))
        self.assertIn("physics", found[0].message)

    def test_nested_source_files_are_scanned(self):
        ws = Workspace(self, {"jobs": ([], {})})
        (ws.dir / "jobs" / "src" / "sub").mkdir()
        (ws.dir / "jobs" / "src" / "sub" / "mod.rs").write_text("use std::fs;", encoding="utf-8")
        self.assertEqual(["banned-use"], rules(ws.check()))

    def test_a_missing_crate_folder_is_an_error_not_a_pass(self):
        ws = Workspace(self, {"jobs": ([], {})})
        __import__("shutil").rmtree(ws.dir / "jobs")
        with self.assertRaises(ac.ArchError):
            ws.check()


class Config(unittest.TestCase):
    def test_a_layer_outside_0_to_4_is_refused(self):
        with self.assertRaises(ValueError):
            ac.load_config('[layers]\nbase = 7\n[rules]\n')

    def test_a_bad_regex_is_refused(self):
        with self.assertRaises(ValueError):
            ac.load_config(CONFIG + '\n[[banned_use]]\npattern = "("\nreason = "x"\n')

    def test_a_banned_use_without_reason_is_refused(self):
        with self.assertRaises(ValueError):
            ac.load_config(CONFIG + '\n[[banned_use]]\npattern = "x"\n')

    def test_an_empty_layer_map_is_refused(self):
        with self.assertRaises(ValueError):
            ac.load_config('[layers]\n[rules]\n')


REAL = (REPO / "gates" / "architecture.toml").read_text(encoding="utf-8")


def banned(test, text, crate="base", name="lib.rs"):
    return rules(make(test, {crate: ([], {name: text})}, REAL))


class EvasionOfBannedUse(unittest.TestCase):
    """Review 0c-2 F1-F5: spellings that must not get past the source scan."""

    def test_grouped_and_aliased_and_spaced_std_imports_are_caught(self):
        for text in ("use std::{fs, io};", "use std::{\n    fs,\n};", "use std::{io::Read, fs::File};", "use std :: fs :: read;",
                     "std\n::fs::read(p);", "use std as s;", "use std::{self as s};", "extern crate std as s;", "use std::*;",
                     "use ::std::fs;", "use std::fs as f;", "use std::{env, collections::BTreeMap};"):
            with self.subTest(text=text):
                self.assertEqual(["banned-use"], banned(self, text))

    def test_allowed_std_imports_are_not_flagged(self):
        for text in ("use std::collections::{BTreeMap, VecDeque};", "use std::sync::{Arc, Mutex, atomic::AtomicU32};", "use std::{fmt, mem};"):
            with self.subTest(text=text):
                self.assertEqual([], banned(self, text))

    def test_threads_and_clocks_in_every_spelling_are_caught(self):
        for text in ("use std::thread::spawn;", "std::thread::Builder::new().spawn(f);", "std::thread::scope(|s| {});",
                     "use std::thread::{self, spawn};", "use std::time::Instant as I;", "let t = SystemTime::now();", "use std::thread::*;"):
            with self.subTest(text=text):
                self.assertEqual(["banned-use"], banned(self, text, crate="ecs"))

    def test_the_job_system_may_spawn_threads(self):
        self.assertEqual([], banned(self, "std::thread::spawn(f);", crate="jobs"))

    def test_libm_floats_in_every_spelling_are_caught_in_a_simulation_crate(self):
        for text in ("x.asin()", "x.sin_cos()", "x.tanh()", "x.log2()", "x.exp2()", "x.cbrt()", "f32::sin(x)", "x.sin ()", "x\n    .sin\n    ()",
                     "x.atan()", "x.ln_1p()", "f64::powf(a, b)"):
            with self.subTest(text=text):
                self.assertEqual(["banned-use"], banned(self, text, crate="physics"))

    def test_exact_float_operations_are_allowed_in_a_simulation_crate(self):
        self.assertEqual([], banned(self, "x.sqrt() + x.abs() + x.floor() + x.min(y)", crate="physics"))

    def test_unstable_sorts_and_selection_are_caught_in_a_simulation_crate(self):
        for text in ("v.sort_unstable();", "v.sort_unstable_by_key(f);", "v.select_nth_unstable(3);"):
            with self.subTest(text=text):
                self.assertEqual(["banned-use"], banned(self, text, crate="physics"))

    def test_byte_and_c_raw_strings_do_not_hide_code(self):
        for opener in ('br"\\"', 'cr"\\"', 'br#"x"#', 'r"\\"'):
            with self.subTest(opener=opener):
                self.assertEqual(["banned-use"], banned(self, f'let a = {opener}; use std::fs; let b = "";'))

    def test_nested_block_comments_hide_only_comment_text(self):
        self.assertEqual([], banned(self, "/* a /* use std::fs; */ use std::io; */"))
        self.assertEqual(["banned-use"], banned(self, "/* a /* b */ use std::fs; */ use std::fs;"))

    def test_path_attributes_and_include_are_refused(self):
        for text in ('#[path = "../x.rs"] mod x;', '#[ path="../x.rs" ]\nmod x;', 'include!("../x.rs");'):
            with self.subTest(text=text):
                self.assertEqual(["banned-use"], banned(self, text))

    def test_build_scripts_tests_and_benches_are_scanned(self):
        ws = Workspace(self, {"base": ([], {})})
        for folder, name in (("", "build.rs"), ("tests", "t.rs"), ("benches", "b.rs"), ("examples", "e.rs")):
            target = ws.dir / "base" / folder
            target.mkdir(exist_ok=True)
            (target / name).write_text("use std::fs;", encoding="utf-8")
        self.assertEqual(4, len(ws.check(REAL)))

    def test_a_nested_crate_folder_is_not_scanned_as_part_of_its_parent(self):
        ws = Workspace(self, {"base": ([], {})})
        (ws.dir / "base" / "inner").mkdir()
        (ws.dir / "base" / "inner" / "Cargo.toml").write_text("", encoding="utf-8")
        (ws.dir / "base" / "inner" / "x.rs").write_text("use std::fs;", encoding="utf-8")
        self.assertEqual([], ws.check(REAL))

    def test_the_crate_and_allow_lists_accept_globs(self):
        config = CONFIG.replace('crates = ["physics"]', 'crates = ["phys*"]').replace('allow_in = ["platform"]', 'allow_in = ["plat*"]')
        self.assertEqual(["banned-use"], rules(make(self, {"physics": ([], {"lib.rs": "x.sin()"})}, config)))
        self.assertEqual([], make(self, {"platform": ([], {"lib.rs": "use std::fs;"})}, config))


class GraphEvasion(unittest.TestCase):
    """Review 0c-2 F6-F12."""

    def test_a_registry_crate_named_like_a_workspace_crate_is_external(self):
        found = make(self, {"render": ([dep("ecs", external=True)], {}), "ecs": ([], {})})
        self.assertEqual(["external"], rules(found))

    def test_a_checked_crate_depending_on_an_exempt_crate_is_a_violation(self):
        found = make(self, {"base": ([dep("skeleton")], {}), "skeleton": ([], {})})
        self.assertEqual(["exempt-dependency"], rules(found))

    def test_overlapping_globs_give_the_lowest_layer(self):
        config = ac.load_config(CONFIG.replace('"app_*" = 4', '"app_*" = 4\n"*_x" = 1'))
        self.assertEqual(1, ac.layer_of("app_x", config))
        config = ac.load_config(CONFIG.replace('"app_*" = 4', '"app_*" = 4\n"app_core" = 4\n"*_core" = 2'))
        self.assertEqual(4, ac.layer_of("app_core", config), "an exact entry wins over globs")

    def test_a_forbidden_edge_is_refused_transitively(self):
        found = make(self, {"physics": ([dep("jobs")], {}), "jobs": ([dep("platform")], {}), "platform": ([dep("ecs")], {}), "ecs": ([], {})})
        self.assertIn("forbidden-edge", rules(found))

    def test_two_separate_cycles_are_two_findings(self):
        found = make(self, {"ecs": ([dep("jobs")], {}), "jobs": ([dep("ecs")], {}), "physics": ([dep("render")], {}), "render": ([dep("physics")], {})})
        self.assertEqual(2, len([finding for finding in found if finding.rule == "cycle"]))

    def test_a_dev_dependency_does_not_make_a_cycle(self):
        found = make(self, {"render": ([dep("adapter_null", "dev")], {}), "adapter_null": ([dep("render")], {})})
        self.assertNotIn("cycle", rules(found))

    def test_a_crate_folder_missing_from_the_workspace_is_a_violation(self):
        ws = Workspace(self, {"base": ([], {})})
        (ws.dir / "stray").mkdir()
        (ws.dir / "stray" / "Cargo.toml").write_text("", encoding="utf-8")
        ws.metadata["workspace_root"] = str(ws.dir)
        self.assertEqual(["unlisted-crate"], rules(ws.check()))

    def test_a_workspace_with_every_folder_listed_is_clean(self):
        ws = Workspace(self, {"base": ([], {})})
        (ws.dir / "base" / "Cargo.toml").write_text("", encoding="utf-8")
        (ws.dir / "target").mkdir()
        (ws.dir / "target" / "Cargo.toml").write_text("", encoding="utf-8")
        ws.metadata["workspace_root"] = str(ws.dir)
        self.assertEqual([], ws.check())


def run_main(*args):
    lines = []
    code = ac.main(list(args), print_line=lines.append)
    return code, lines


class Main(unittest.TestCase):
    def write(self, crates):
        ws = Workspace(self, crates)
        meta = ws.dir / "meta.json"
        meta.write_text(json.dumps(ws.metadata), encoding="utf-8")
        cfg = ws.dir / "arch.toml"
        cfg.write_text(CONFIG, encoding="utf-8")
        return meta, cfg

    def test_seeded_violation_fails_with_a_readable_report(self):
        meta, cfg = self.write({"jobs": ([dep("render")], {"lib.rs": "use std::fs;"}), "render": ([], {})})
        code, lines = run_main("--config", str(cfg), "--metadata", str(meta))
        self.assertEqual(1, code)
        text = "\n".join(lines)
        self.assertIn("VIOLATION layer", text)
        self.assertIn("VIOLATION banned-use", text)

    def test_a_clean_workspace_exits_zero(self):
        meta, cfg = self.write({"base": ([], {"lib.rs": ""})})
        code, lines = run_main("--config", str(cfg), "--metadata", str(meta))
        self.assertEqual(0, code)
        self.assertIn("OK", lines[-1])

    def test_a_missing_config_or_metadata_file_fails(self):
        meta, cfg = self.write({"base": ([], {})})
        self.assertEqual(1, run_main("--config", str(cfg) + ".none", "--metadata", str(meta))[0])
        self.assertEqual(1, run_main("--config", str(cfg), "--metadata", str(meta) + ".none")[0])

    def test_unreadable_metadata_fails(self):
        meta, cfg = self.write({"base": ([], {})})
        meta.write_text("not json", encoding="utf-8")
        self.assertEqual(1, run_main("--config", str(cfg), "--metadata", str(meta))[0])

    def test_metadata_without_packages_fails(self):
        meta, cfg = self.write({"base": ([], {})})
        meta.write_text('{"packages": []}', encoding="utf-8")
        self.assertEqual(1, run_main("--config", str(cfg), "--metadata", str(meta))[0])


class RealWorkspace(unittest.TestCase):
    def test_the_real_workspace_passes_the_real_rules(self):
        env = {**os.environ, "PATH": os.environ["PATH"] + os.pathsep + str(Path.home() / ".cargo" / "bin")}
        done = subprocess.run(["cargo", "metadata", "--format-version", "1", "--no-deps", "--offline"], cwd=REPO, env=env,
                              capture_output=True, check=True)
        meta = Path(tempfile.mkdtemp()) / "meta.json"
        meta.write_bytes(done.stdout)
        self.addCleanup(meta.unlink)
        code, lines = run_main("--config", str(REPO / "gates" / "architecture.toml"), "--metadata", str(meta))
        self.assertEqual((0, []), (code, lines[:-1]))

    def test_every_crate_mapped_in_the_real_rules_exists_in_the_wiki_module_map(self):
        config = ac.load_config((REPO / "gates" / "architecture.toml").read_text(encoding="utf-8"))
        overview = (REPO / "wiki" / "architecture" / "overview.md").read_text(encoding="utf-8")
        for name in config.layers:
            self.assertIn(f"`{name.rstrip('*')}", overview)


if __name__ == "__main__":
    unittest.main()
