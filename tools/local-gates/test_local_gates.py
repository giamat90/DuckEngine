import tempfile
import unittest
from pathlib import Path

import local_gates as lg


def tree(test, files):
    root = Path(tempfile.mkdtemp())
    test.addCleanup(lambda: __import__("shutil").rmtree(root, ignore_errors=True))
    for relative in files:
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("", encoding="utf-8")
    return root


def names(steps):
    return [step.name for step in steps]


class Plan(unittest.TestCase):
    def test_rust_steps_mirror_the_ci_commands(self):
        commands = [" ".join(step.argv) for step in lg.rust_steps()]
        self.assertIn("cargo fmt --all --check", commands)
        self.assertIn("cargo clippy --workspace --all-targets --locked -- -D warnings", commands)
        self.assertIn("cargo test --workspace --locked", commands)
        self.assertIn("cargo clippy -p app_runtime --no-default-features --all-targets --locked -- -D warnings", commands)
        self.assertIn("cargo test -p app_runtime --no-default-features --locked", commands)

    def test_gate_steps_run_the_gate_tools_on_the_working_tree(self):
        commands = [" ".join(step.argv) for step in lg.gate_steps("origin/main")]
        for tool in ("arch_check.py", "unsafe_budget.py", "wiki_lint.py", "check_digest.py"):
            self.assertTrue(any(tool in command for command in commands), tool)
        self.assertTrue(any("diff_hash.py compute --base origin/main --head HEAD" in command for command in commands))

    def test_each_tool_directory_with_tests_gets_one_unittest_step(self):
        root = tree(self, ["tools/a/test_x.py", "tools/a/test_y.py", "tools/a/x.py", "tools/b/test_z.py", "tools/c/z.py", "acceptance/test_validate.py"])
        steps = lg.python_test_steps(root)
        self.assertEqual(["tests acceptance", "tests tools/a", "tests tools/b"], names(steps))
        by_name = {step.name: step for step in steps}
        self.assertEqual(["python", "-m", "unittest", "-q", "test_x", "test_y"], by_name["tests tools/a"].argv)
        self.assertEqual(root / "tools" / "a", by_name["tests tools/a"].cwd)

    def test_cargo_deny_is_planned_only_when_installed(self):
        self.assertEqual([], lg.deny_steps(lambda name: None))
        self.assertEqual(["cargo deny"], names(lg.deny_steps(lambda name: "/bin/cargo-deny")))

    def test_the_wsl_leg_converts_the_path_and_uses_its_own_target_directory(self):
        steps = lg.wsl_steps(Path("C:/Work/Duck"))
        self.assertEqual(["wsl cargo test"], names(steps))
        text = " ".join(steps[0].argv)
        self.assertIn("/mnt/c/Work/Duck", text)
        self.assertIn("CARGO_TARGET_DIR=/root/target-gates", text)

    def test_build_plan_leaves_the_wsl_leg_out_unless_asked(self):
        root = tree(self, ["tools/a/test_x.py"])
        plain = names(lg.build_plan(root, "origin/main", wsl=False, which=lambda name: None))
        self.assertNotIn("wsl cargo test", plain)
        with_wsl = names(lg.build_plan(root, "origin/main", wsl=True, which=lambda name: None))
        self.assertIn("wsl cargo test", with_wsl)


class Run(unittest.TestCase):
    def steps(self):
        return [lg.Step("one", Path("."), ["a"]), lg.Step("two", Path("."), ["b"]), lg.Step("three", Path("."), ["c"])]

    def test_every_step_runs_even_after_a_failure_and_the_exit_code_is_one(self):
        ran = []

        def execute(step):
            ran.append(step.name)
            return 1 if step.name == "two" else 0

        results = lg.run_plan(self.steps(), execute, print_line=lambda line: None)
        self.assertEqual(["one", "two", "three"], ran)
        self.assertEqual([("one", "pass"), ("two", "FAIL"), ("three", "pass")], [(r.name, r.status) for r in results])
        self.assertEqual(1, lg.exit_code(results))

    def test_all_green_exits_zero(self):
        results = lg.run_plan(self.steps(), lambda step: 0, print_line=lambda line: None)
        self.assertEqual(0, lg.exit_code(results))

    def test_a_missing_program_is_a_failure_not_a_crash(self):
        results = lg.run_plan(self.steps()[:1], lambda step: lg.MISSING, print_line=lambda line: None)
        self.assertEqual("FAIL", results[0].status)

    def test_the_summary_names_what_was_not_run(self):
        lines = []
        lg.run_plan(self.steps()[:1], lambda step: 0, print_line=lines.append)
        self.assertTrue(any("not run locally" in line and "Miri" in line for line in lines))


class Resolve(unittest.TestCase):
    def test_the_program_is_looked_up_on_the_search_path_given_not_the_parents(self):
        bin_dir = tree(self, ["fake-tool.exe", "fake-tool"])
        for fake in bin_dir.iterdir():  # `which` wants the executable bit on Linux
            fake.chmod(0o755)
        argv = lg.resolve(["fake-tool", "--x"], str(bin_dir))
        self.assertEqual("--x", argv[1])
        self.assertTrue(Path(argv[0]).is_file())

    def test_an_unknown_program_is_left_as_it_is(self):
        self.assertEqual(["no-such-program-xyz", "a"], lg.resolve(["no-such-program-xyz", "a"], ""))


class Cli(unittest.TestCase):
    def test_list_prints_the_plan_without_running_it(self):
        root = tree(self, ["tools/a/test_x.py"])
        lines = []
        code = lg.main(["--repo", str(root), "--list"], print_line=lines.append)
        self.assertEqual(0, code)
        self.assertTrue(any("cargo fmt" in line for line in lines))


if __name__ == "__main__":
    unittest.main()
