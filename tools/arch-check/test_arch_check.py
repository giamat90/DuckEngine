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


THREAD_CONFIG = CONFIG + r"""
[[banned_use]]
pattern = '\bthread::spawn\b'
reason = "direct thread creation"
allow_in = ["jobs"]
allow_in_tests = true
"""

SPAWN = "std::thread::spawn(|| {});"


class AllowInTests(unittest.TestCase):
    """Card 1-4: a rule marked allow_in_tests does not apply to test-only code."""

    def found(self, text, files=None, crate="base"):
        ws = Workspace(self, {crate: ([], files if files is not None else {"lib.rs": text})})
        return rules(ws.check(THREAD_CONFIG))

    def test_a_cfg_test_module_may_spawn_threads(self):
        self.assertEqual([], self.found("#[cfg(test)]\nmod tests {\n    fn t() { " + SPAWN + " }\n}\n"))

    def test_production_code_next_to_a_test_module_still_fails(self):
        text = "fn run() { " + SPAWN + " }\n#[cfg(test)]\nmod tests {\n    fn t() { " + SPAWN + " }\n}\n"
        self.assertEqual(["banned-use"], self.found(text))

    def test_code_after_the_test_module_still_fails(self):
        text = "#[cfg(test)]\nmod tests { fn t() { " + SPAWN + " } }\nfn run() { " + SPAWN + " }\n"
        self.assertEqual(["banned-use"], self.found(text))

    def test_cfg_not_test_is_not_a_test(self):
        self.assertEqual(["banned-use"], self.found("#[cfg(not(test))]\nfn run() { " + SPAWN + " }\n"))

    def test_other_cfg_conditions_are_not_a_test(self):
        for cond in ("any(test, unix)", 'feature = "test"', "all(test, unix)"):
            self.assertEqual(["banned-use"], self.found("#[cfg(" + cond + ")]\nfn run() { " + SPAWN + " }\n"), cond)

    def test_a_cfg_test_function_and_a_cfg_test_use_are_test_code(self):
        self.assertEqual([], self.found("#[cfg(test)]\n#[allow(dead_code)]\nfn helper() { " + SPAWN + " }\n"))
        self.assertEqual([], self.found("#[cfg(test)]\nuse std::thread::spawn;\nfn run() {}\n"), "a use item ends at the semicolon")

    def test_nested_braces_in_the_test_module_do_not_end_it_early(self):
        text = "#[cfg(test)]\nmod tests {\n    fn a() { if true { } }\n    fn b() { " + SPAWN + " }\n}\n"
        self.assertEqual([], self.found(text))

    def test_braces_in_strings_and_comments_do_not_confuse_the_extent(self):
        text = '#[cfg(test)]\nmod tests { fn a() { let s = "}"; /* } */ }\n    fn b() { ' + SPAWN + " } }\nfn run() { " + SPAWN + " }\n"
        self.assertEqual(["banned-use"], self.found(text))

    def test_an_inner_cfg_test_attribute_makes_the_whole_file_test_code(self):
        files = {"lib.rs": "mod t;", "t.rs": "#![cfg(test)]\nfn t() { " + SPAWN + " }\n"}
        self.assertEqual([], self.found("", files=files))

    def test_files_in_the_tests_folder_are_test_code(self):
        ws = Workspace(self, {"base": ([], {"lib.rs": ""})})
        (ws.dir / "base" / "tests").mkdir()
        (ws.dir / "base" / "tests" / "threads.rs").write_text("fn t() { " + SPAWN + " }", encoding="utf-8")
        self.assertEqual([], rules(ws.check(THREAD_CONFIG)))

    def test_a_folder_called_tests_inside_src_is_not_exempt(self):
        ws = Workspace(self, {"base": ([], {"lib.rs": ""})})
        (ws.dir / "base" / "src" / "tests").mkdir()
        (ws.dir / "base" / "src" / "tests" / "mod.rs").write_text("fn t() { " + SPAWN + " }", encoding="utf-8")
        self.assertEqual(["banned-use"], rules(ws.check(THREAD_CONFIG)))

    def test_an_out_of_line_test_module_is_not_exempt(self):
        files = {"lib.rs": "#[cfg(test)]\nmod tests;\n", "tests.rs": "fn t() { " + SPAWN + " }\n"}
        self.assertEqual(["banned-use"], self.found("", files=files))

    def test_a_cfg_test_field_does_not_exempt_the_code_after_its_struct(self):
        text = "struct S {\n    #[cfg(test)]\n    x: u8,\n    y: u8,\n}\nfn run() { " + SPAWN + " }\n"
        self.assertEqual(["banned-use"], self.found(text))

    def test_a_cfg_test_match_arm_or_argument_does_not_exempt_the_arms_after_it(self):
        text = "fn run(v: u8) {\n    match v {\n        #[cfg(test)]\n        0 => 1,\n        _ => " + SPAWN[:-1] + ",\n    }\n}\n"
        self.assertEqual(["banned-use"], self.found(text))

    def test_a_cfg_test_statement_ends_at_its_semicolon(self):
        text = "fn run() {\n    #[cfg(test)]\n    let _t = 1;\n    " + SPAWN + "\n}\n"
        self.assertEqual(["banned-use"], self.found(text))

    def test_commas_inside_parentheses_do_not_end_a_cfg_test_function(self):
        self.assertEqual([], self.found("#[cfg(test)]\nfn t(a: u8, b: u8) { " + SPAWN + " }\n"))

    def test_an_inner_attribute_inside_a_macro_body_does_not_exempt_the_file(self):
        text = "macro_rules! u { () => { #![cfg(test)] }; }\npub fn r() { " + SPAWN + " }\n"
        self.assertEqual(["banned-use"], self.found(text))

    def test_an_inner_attribute_inside_an_inline_module_does_not_exempt_the_file(self):
        text = "mod m {\n    #![cfg(test)]\n}\npub fn r() { " + SPAWN + " }\n"
        self.assertEqual(["banned-use"], self.found(text))

    def test_an_inner_attribute_after_other_inner_attributes_still_counts(self):
        text = "#![allow(dead_code)]\n#![cfg(test)]\nfn t() { " + SPAWN + " }\n"
        self.assertEqual([], self.found(text))

    def test_an_inner_attribute_after_an_item_does_not_count(self):
        text = "fn first() {}\n#![cfg(test)]\nfn r() { " + SPAWN + " }\n"
        self.assertEqual(["banned-use"], self.found(text))

    def test_production_code_on_the_same_line_as_a_test_item_still_fails(self):
        for text in ("#[cfg(test)] fn t() {} fn r() { " + SPAWN + " }\n",
                     "#[cfg(test)]\nmod t {\n}  pub fn r() { " + SPAWN + " }\n",
                     "pub fn r() { #[cfg(test)] {} " + SPAWN + " }\n",
                     "#[cfg(test)] use std::thread; pub fn r() { " + SPAWN + " }\n"):
            self.assertEqual(["banned-use"], self.found(text), text)

    def test_a_test_item_on_a_line_of_its_own_with_a_use_in_it_is_still_exempt(self):
        self.assertEqual([], self.found("#[cfg(test)] mod t { use std::thread::spawn; fn f() { " + SPAWN + " } }\n"))

    def test_a_file_in_tests_that_a_non_test_target_builds_is_not_exempt(self):
        ws = Workspace(self, {"base": ([], {"lib.rs": ""})})
        real = ws.dir / "base" / "tests" / "real.rs"
        real.parent.mkdir()
        real.write_text("pub fn r() { " + SPAWN + " }", encoding="utf-8")
        ws.packages[0]["targets"] = [{"kind": ["lib"], "src_path": str(real)}]
        self.assertEqual(["banned-use"], rules(ws.check(THREAD_CONFIG)))

    def test_a_test_target_in_tests_stays_exempt(self):
        ws = Workspace(self, {"base": ([], {"lib.rs": ""})})
        case = ws.dir / "base" / "tests" / "case.rs"
        case.parent.mkdir()
        case.write_text("fn t() { " + SPAWN + " }", encoding="utf-8")
        ws.packages[0]["targets"] = [{"kind": ["test"], "src_path": str(case)}]
        self.assertEqual([], rules(ws.check(THREAD_CONFIG)))

    def test_other_rules_are_not_relaxed_in_test_code(self):
        text = "#[cfg(test)]\nmod tests { fn t() { use std::fs; } }\n"
        self.assertEqual(["banned-use"], self.found(text))

    def test_a_rule_without_the_key_is_not_relaxed(self):
        config = THREAD_CONFIG.replace("allow_in_tests = true", "")
        ws = Workspace(self, {"base": ([], {"lib.rs": "#[cfg(test)]\nmod tests { fn t() { " + SPAWN + " } }\n"})})
        self.assertEqual(["banned-use"], rules(ws.check(config)))

    def test_allow_in_tests_must_be_a_boolean(self):
        with self.assertRaises(ValueError):
            ac.load_config(CONFIG + '\n[[banned_use]]\npattern = "x"\nreason = "x"\nallow_in_tests = "yes"\n')

    def test_the_line_in_the_message_is_the_production_line(self):
        text = "#[cfg(test)]\nmod tests { fn t() { " + SPAWN + " } }\n\nfn run() { " + SPAWN + " }\n"
        ws = Workspace(self, {"base": ([], {"lib.rs": text})})
        self.assertIn("lib.rs:4", ws.check(THREAD_CONFIG)[0].message)


class RealThreadRule(unittest.TestCase):
    """Card 1-7: the thread rule of the real gate configuration, including module aliases."""

    CALL = "fn run() { th::spawn(|| {}); }\n"

    def found(self, text):
        config = ac.load_config((REPO / "gates" / "architecture.toml").read_text(encoding="utf-8"))
        ws = Workspace(self, {"base": ([], {"lib.rs": text})})
        return rules(ac.check(ws.metadata, config))

    def test_a_module_alias_of_std_thread_is_refused(self):
        for use in ("use std::thread as th;", "use std::thread::{self as th};", "use std::{thread as th};",
                    "use ::std::thread as th;", "pub use std::thread as th;",
                    "use std::{sync::Arc, thread::{self as th}};", "use  std :: thread  as  th ;"):
            self.assertEqual(["banned-use"], self.found(use + "\n" + self.CALL), use)

    def test_a_rename_of_any_module_called_thread_is_refused(self):
        for text in ("use std::thread;\nuse self::thread as th;\n" + self.CALL, "use thread as th;\n" + self.CALL,
                     "use super::thread as th;\n", "use crate::thread as th;\n", "use thread::{self as th};\n",
                     "mod m { pub use std::thread; }\nuse m::thread as th;\n" + self.CALL):
            self.assertEqual(["banned-use"], self.found(text), text)

    def test_raw_identifiers_do_not_hide_a_thread(self):
        for text in ("use std::r#thread as th;\n" + self.CALL, "fn f() { std::thread::r#spawn(|| {}); }",
                     "fn f() { std::r#thread::spawn(|| {}); }", "use std::thread::r#spawn as go;\n",
                     "fn f() { std::thread::r#Builder::new(); }"):
            self.assertEqual(["banned-use"], self.found(text), text)

    def test_a_raw_identifier_does_not_hide_another_banned_module_either(self):
        self.assertEqual(["banned-use"], self.found("fn f() { std::r#fs::read(p); }"))

    def test_raw_strings_are_still_blanked_and_raw_identifiers_elsewhere_are_harmless(self):
        self.assertEqual([], self.found('const S: &str = r#"std::thread::spawn"#;\nfn f() { let r#type = 1; }\n'))

    def test_the_module_alias_case_with_nothing_else_banned_in_the_line(self):
        self.assertEqual(["banned-use"], self.found("use std::{sync::Arc, thread as th};\n" + self.CALL))

    def test_an_alias_of_a_thread_creation_item_is_refused(self):
        for use in ("use std::thread::spawn as go;", "use std::thread::Builder as B;", "use std::thread::scope as s;",
                    "use std::thread::{self, spawn as go};"):
            self.assertEqual(["banned-use"], self.found(use + "\n"), use)

    def test_plain_thread_creation_is_still_refused(self):
        for text in ("fn r() { std::thread::spawn(|| {}); }", "use std::thread;\nfn r() { thread::spawn(|| {}); }",
                     "use std::thread::*;"):
            self.assertEqual(["banned-use"], self.found(text), text)

    def test_the_harmless_thread_items_stay_allowed(self):
        for text in ("use std::thread;\nfn r() { thread::sleep(d); }", "use std::thread::{sleep, current, yield_now};",
                     "use std::thread::sleep as nap;", "use std::thread::available_parallelism as cores;",
                     "fn r() { std::thread::yield_now(); }"):
            self.assertEqual([], self.found(text), text)

    def test_an_unrelated_module_called_th_is_allowed(self):
        self.assertEqual([], self.found("mod th { pub fn spawn() {} }\n" + self.CALL))

    def test_a_module_alias_in_test_code_is_allowed(self):
        self.assertEqual([], self.found("#[cfg(test)]\nmod tests {\n    use std::thread as th;\n    fn t() { th::spawn(|| {}); }\n}\n"))


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

    def test_libm_floats_in_every_spelling_are_caught_in_the_physics_crate(self):
        for text in ("x.asin()", "x.sin_cos()", "x.tanh()", "x.log2()", "x.exp2()", "x.cbrt()", "f32::sin(x)", "x.sin ()", "x\n    .sin\n    ()",
                     "x.atan()", "x.ln_1p()", "f64::powf(a, b)", "<f32>::sin(x)", "<f32 as Float>::cos(x)", "x.mul_add(a, b)", "f32::mul_add(a, b, c)", "x.powi(2)"):
            with self.subTest(text=text):
                self.assertEqual(["banned-use"], banned(self, text, crate="physics"))

    def test_libm_floats_are_allowed_outside_physics_because_the_math_facade_lives_in_base(self):
        for text in ("x.sin()", "x.exp()", "f32::cos(x)", "f64::powf(a, b)", "x.mul_add(a, b)"):
            with self.subTest(text=text):
                self.assertEqual([], banned(self, text, crate="base"))
                self.assertEqual([], banned(self, text, crate="render"))

    def test_estimate_intrinsics_are_caught_in_physics_and_base(self):
        for text in ("_mm_rsqrt_ps(a)", "_mm_rcp_ps(a)", "_mm_rsqrt_ss(a)", "vrsqrteq_f32(a)", "vrecpeq_f32(a)", "vrsqrtsq_f32(a, b)", "vrecpsq_f32(a, b)"):
            for crate in ("physics", "base"):
                with self.subTest(text=text, crate=crate):
                    self.assertEqual(["banned-use"], banned(self, text, crate=crate))

    def test_fused_intrinsics_are_caught_in_physics(self):
        for text in ("_mm_fmadd_ps(a, b, c)", "_mm256_fnmsub_ps(a, b, c)", "vfmaq_f32(a, b, c)", "vfmsq_f32(a, b, c)", "vmlaq_f32(a, b, c)", "vfmaq_n_f32(a, b, c)",
                     "fma(a, b, c)", "fmaf(a, b, c)", "libm::fmaf(a, b, c)", "fmuladdf32(a, b, c)"):
            with self.subTest(text=text):
                self.assertEqual(["banned-use"], banned(self, text, crate="physics"))

    def test_fused_intrinsics_are_allowed_in_base_where_the_math_facade_lives(self):
        for text in ("_mm_fmadd_ps(a, b, c)", "vfmaq_f32(a, b, c)", "fmaf(a, b, c)"):
            with self.subTest(text=text):
                self.assertEqual([], banned(self, text, crate="base"))

    def test_the_facade_function_madd_is_allowed_in_physics_and_integer_multiply_accumulate_is_not_flagged(self):
        self.assertEqual([], banned(self, "let y = madd(a, b, c); let z = base::math::madd(a, b, c);", crate="physics"))
        self.assertEqual([], banned(self, "vmlal_s16(a, b, c); vmlaq_s32(a, b, c);", crate="physics"))

    def test_exact_intrinsics_and_lookalike_names_are_not_flagged_in_physics_and_base(self):
        text = "_mm_add_ps(a, b); _mm_sqrt_ps(a); _mm_div_ps(a, b); vaddq_f32(a, b); let vfmt = 1; fmt::format(a); let fmax = 2;"
        for crate in ("physics", "base"):
            with self.subTest(crate=crate):
                self.assertEqual([], banned(self, text, crate=crate))

    def test_a_method_named_like_a_libm_function_is_a_known_false_positive_in_physics(self):
        # The scan matches the method name, not the receiver type: `logger.log(..)` in physics is refused.
        # Accepted: physics logs through the engine's log port, not a method named `log`.
        self.assertEqual(["banned-use"], banned(self, "logger.log(Level::Info);", crate="physics"))

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


class SourceScope(unittest.TestCase):
    """Review 0c-7 A1, A2: which files are scanned."""

    def test_a_target_folder_inside_src_is_scanned(self):
        ws = Workspace(self, {"base": ([], {})})
        (ws.dir / "base" / "src" / "target").mkdir()
        (ws.dir / "base" / "src" / "target" / "mod.rs").write_text("use std::fs;", encoding="utf-8")
        self.assertEqual(["banned-use"], rules(ws.check(REAL)))

    def test_the_crate_build_output_folder_is_not_scanned(self):
        ws = Workspace(self, {"base": ([], {})})
        (ws.dir / "base" / "target" / "debug").mkdir(parents=True)
        (ws.dir / "base" / "target" / "debug" / "gen.rs").write_text("use std::fs;", encoding="utf-8")
        self.assertEqual([], ws.check(REAL))

    def with_target(self, test_crates, src_path):
        ws = Workspace(self, test_crates)
        ws.packages[0]["targets"] = [{"name": "x", "src_path": str(src_path(ws))}]
        return ws.check(REAL)

    def test_a_target_whose_source_is_outside_the_crate_folder_is_a_finding(self):
        found = self.with_target({"base": ([], {})}, lambda ws: ws.dir / "elsewhere" / "lib.rs")
        self.assertEqual(["src-outside"], rules(found))
        self.assertIn("base", found[0].message)

    def test_a_dot_dot_path_that_leaves_the_crate_is_a_finding(self):
        found = self.with_target({"base": ([], {})}, lambda ws: ws.dir / "base" / "src" / ".." / ".." / "other" / "lib.rs")
        self.assertEqual(["src-outside"], rules(found))

    def test_targets_inside_the_crate_folder_are_fine(self):
        found = self.with_target({"base": ([], {"lib.rs": ""})}, lambda ws: ws.dir / "base" / "src" / "lib.rs")
        self.assertEqual([], found)

    def test_an_adapter_may_have_its_source_elsewhere(self):
        found = self.with_target({"adapter_gl": ([], {})}, lambda ws: ws.dir / "elsewhere" / "lib.rs")
        self.assertEqual([], found)

    def test_a_package_without_targets_is_still_checked(self):
        self.assertEqual([], make(self, {"base": ([], {"lib.rs": ""})}, REAL))


class IncludeAndPathVariants(unittest.TestCase):
    """Review 0c-7 A3: every way to pull in a file the scan cannot follow."""

    def test_other_spellings_are_refused(self):
        for text in ("include!{\"../x.rs\"}", "include !(\"../x.rs\");", "include\n!(\"../x.rs\");", "std::include!(\"x.rs\");",
                     "core::include ! [\"x.rs\"];", "#[cfg_attr(all(), path = \"../x.rs\")] mod x;", "#[cfg_attr(unix, path=\"x.rs\")]\nmod x;",
                     "#![path = \"x.rs\"]", "#[ path\n= \"x.rs\" ] mod x;"):
            with self.subTest(text=text):
                self.assertEqual(["banned-use"], banned(self, text))

    def test_embedding_data_and_unrelated_names_stay_allowed(self):
        for text in ('const A: &str = include_str!("a.txt");', 'const B: &[u8] = include_bytes!("b.bin");',
                     "let path = 1; let p = path;", "fn include() {} include();", "let x = Path::new(a).path();"):
            with self.subTest(text=text):
                self.assertEqual([], banned(self, text))


class ForeignCodeAndDeterminism(unittest.TestCase):
    """Review 0c-7 A4: extern blocks, link attributes, std::ffi, std::arch, inline assembly, math."""

    def test_extern_blocks_are_refused_in_layers_0_to_2(self):
        for text in ('extern "C" { fn f(); }', 'unsafe extern "system" {\n fn f();\n}', "extern{ fn f(); }", 'extern\n"C"\n{ }',
                     'extern r"C" { }', 'extern "C"{}'):
            with self.subTest(text=text):
                self.assertEqual(["banned-use"], banned(self, text, crate="render"))

    def test_extern_functions_and_crates_are_not_blocks(self):
        for text in ('pub extern "C" fn f() {}', 'type F = extern "C" fn(i32);', "extern crate alloc;"):
            with self.subTest(text=text):
                self.assertEqual([], banned(self, text, crate="render"))

    def test_link_attributes_are_refused(self):
        for text in ('#[link(name = "m")] extern "C" {}', '#[link_name = "x"] fn f();', '#[ link ( name = "m" ) ]', '#[unsafe(link_name = "x")] fn f();'):
            with self.subTest(text=text):
                self.assertIn("banned-use", banned(self, text, crate="render"))

    def test_std_ffi_is_an_os_module_only_the_platform_may_use(self):
        for text in ("use std::ffi::OsString;", "use std::{ffi, mem};", "std::ffi::CStr::from_ptr(p);"):
            with self.subTest(text=text):
                self.assertEqual(["banned-use"], banned(self, text, crate="render"))
        self.assertEqual([], banned(self, "use std::ffi::OsString;", crate="platform"))

    def test_architecture_intrinsics_and_assembly_belong_to_base_only(self):
        for text in ("use std::arch::x86_64::_mm_add_ps;", "core::arch::asm!(\"nop\");", "use core::{arch, mem};", "use std::{arch::x86_64::*};",
                     "asm!(\"nop\");", "global_asm!(\"nop\");", "use std::arch::asm;"):
            with self.subTest(text=text):
                self.assertEqual(["banned-use"], banned(self, text, crate="render"))
                self.assertEqual([], banned(self, text, crate="base"))

    def test_other_core_modules_stay_allowed(self):
        self.assertEqual([], banned(self, "use core::mem; use core::sync::atomic::AtomicU32;", crate="render"))

    def test_base_may_use_methods_named_like_libm_ones_exact_floats_and_hash_containers(self):
        text = "logger.log(Level::Info); self.exp(1); x.sqrt() + x.abs(); let m: HashMap<u32, u32> = HashMap::new(); v.sort_unstable();"
        self.assertEqual([], banned(self, text, crate="base"))

    def test_inequality_tests_on_names_that_look_like_macros_are_not_flagged(self):
        for text in ("if include != 3 {}", "if asm != 3 {}"):
            with self.subTest(text=text):
                self.assertEqual([], banned(self, text, crate="render"))

    def test_hash_maps_are_refused_in_simulation_crates_only(self):
        self.assertIn("banned-use", banned(self, "let m: HashSet<u32> = HashSet::new();", crate="physics"))
        self.assertEqual([], banned(self, "let m: HashSet<u32> = HashSet::new();", crate="render"))


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
