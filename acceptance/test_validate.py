"""Tests for the holdout acceptance scenario validator (card 0c-6)."""

import tempfile
import unittest
from pathlib import Path

import validate

GOOD = """\
id = "example-window-opens"
title = "A window opens and closes"
status = "pending"
phase = "1"
expect = "Starting the sample opens a window; closing it ends the process with exit code 0."
"""
ACTIVE = GOOD.replace('"pending"', '"active"') + 'check = "run the sample headless and compare the replay hash"\n'


class Fixture(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.dir = Path(self._tmp.name)

    def write(self, name, text):
        (self.dir / name).write_text(text, encoding="utf-8")


class Validate(Fixture):
    def test_a_pending_scenario_is_valid_and_counted(self):
        self.write("example-window-opens.toml", GOOD)
        found = validate.validate(self.dir)
        self.assertEqual(found.problems, [])
        self.assertEqual((found.active, found.pending), (0, 1))

    def test_an_active_scenario_needs_a_check(self):
        self.write("example-window-opens.toml", ACTIVE)
        self.assertEqual(validate.validate(self.dir).problems, [])
        self.write("example-window-opens.toml", ACTIVE.replace('check = "run the sample headless and compare the replay hash"\n', ""))
        self.assertTrue(any("check" in p for p in validate.validate(self.dir).problems))

    def test_a_missing_field_is_named(self):
        self.write("example-window-opens.toml", GOOD.replace('title = "A window opens and closes"\n', ""))
        problems = validate.validate(self.dir).problems
        self.assertEqual(len(problems), 1)
        self.assertIn("title", problems[0])

    def test_the_id_must_match_the_file_name(self):
        self.write("other-name.toml", GOOD)
        self.assertTrue(any("id" in p and "other-name" in p for p in validate.validate(self.dir).problems))

    def test_an_id_with_odd_characters_is_refused(self):
        self.write("Bad_Id.toml", GOOD.replace("example-window-opens", "Bad_Id"))
        self.assertTrue(any("id" in p for p in validate.validate(self.dir).problems))

    def test_an_unknown_status_is_refused(self):
        self.write("example-window-opens.toml", GOOD.replace('"pending"', '"done"'))
        self.assertTrue(any("status" in p for p in validate.validate(self.dir).problems))

    def test_an_empty_expectation_is_refused(self):
        self.write("example-window-opens.toml", GOOD.replace("Starting the sample opens a window; closing it ends the process with exit code 0.", "  "))
        self.assertTrue(any("expect" in p for p in validate.validate(self.dir).problems))

    def test_an_unknown_key_is_refused_so_a_typo_cannot_hide_a_field(self):
        self.write("example-window-opens.toml", GOOD + "chek = 1\n")
        self.assertTrue(any("chek" in p for p in validate.validate(self.dir).problems))

    def test_a_field_of_the_wrong_type_is_refused(self):
        self.write("example-window-opens.toml", GOOD.replace('phase = "1"', "phase = 1"))
        self.assertTrue(any("phase" in p for p in validate.validate(self.dir).problems))

    def test_malformed_toml_is_a_problem_not_a_crash(self):
        self.write("example-window-opens.toml", "id = = 1\n")
        self.assertTrue(any("example-window-opens.toml" in p for p in validate.validate(self.dir).problems))

    def test_a_file_that_is_not_utf8_is_a_problem(self):
        (self.dir / "example-window-opens.toml").write_bytes(b"\xff\xfe\x00")
        self.assertEqual(len(validate.validate(self.dir).problems), 1)

    def test_no_scenarios_is_a_problem(self):
        self.assertTrue(any("no scenario" in p for p in validate.validate(self.dir).problems))

    def test_a_missing_directory_is_a_problem(self):
        self.assertTrue(any("not a directory" in p for p in validate.validate(self.dir / "nope").problems))

    def test_a_file_that_is_not_a_scenario_is_a_problem_not_skipped(self):
        self.write("example-window-opens.toml", GOOD)
        for name in ("notes.md", "UP.TOML", "x.toml.txt", "noext", ".hidden.toml"):
            with self.subTest(name=name):
                self.write(name, GOOD)
                self.assertTrue(any(name in p for p in validate.validate(self.dir).problems))
                (self.dir / name).unlink()

    def test_the_readme_is_the_only_allowed_other_file(self):
        self.write("example-window-opens.toml", GOOD)
        self.write("README.md", "rules")
        self.assertEqual(validate.validate(self.dir).problems, [])

    def test_a_symlink_is_refused(self):
        class Link:  # symlinks cannot be created without privileges on Windows
            name = "link.toml"
            suffix = ".toml"

            def is_symlink(self):
                return True

        self.assertIn("symlink", validate.entry_problem(Link()))

    def test_the_size_limit_is_inclusive(self):
        path = self.dir / "example-window-opens.toml"
        filler = b"#" + b"x" * (validate.MAX_BYTES - len(GOOD.encode()) - 2) + b"\n"
        path.write_bytes(GOOD.encode() + filler)  # bytes: write_text would translate newlines on Windows
        self.assertEqual(path.stat().st_size, validate.MAX_BYTES)
        self.assertEqual(validate.validate(self.dir).problems, [])
        path.write_bytes(GOOD.encode() + filler + b"#")
        self.assertTrue(any("larger than" in p for p in validate.validate(self.dir).problems))

    def test_a_hidden_toml_file_is_refused(self):
        self.write("example-window-opens.toml", GOOD)
        self.write(".hidden.toml", GOOD)
        self.assertTrue(any(".hidden.toml" in p and "not a scenario" in p for p in validate.validate(self.dir).problems))

    def test_a_huge_file_is_refused_unread(self):
        self.write("example-window-opens.toml", GOOD + "#" + "x" * validate.MAX_BYTES)
        self.assertTrue(any("larger than" in p for p in validate.validate(self.dir).problems))

    def test_a_meaningless_expectation_or_phase_is_refused(self):
        self.write("example-window-opens.toml", GOOD.replace("Starting the sample opens a window; closing it ends the process with exit code 0.", "x"))
        self.assertTrue(any("expect" in p for p in validate.validate(self.dir).problems))
        self.write("example-window-opens.toml", GOOD.replace('phase = "1"', 'phase = "banana"'))
        self.assertTrue(any("phase" in p for p in validate.validate(self.dir).problems))

    def test_an_active_check_of_only_whitespace_or_wrong_type_is_refused(self):
        for check in ('"   "', "5"):
            with self.subTest(check=check):
                self.write("example-window-opens.toml", GOOD.replace('"pending"', '"active"') + f"check = {check}\n")
                self.assertTrue(any("check" in p for p in validate.validate(self.dir).problems))

    def test_status_of_the_wrong_type_or_an_id_with_a_newline_is_refused(self):
        self.write("example-window-opens.toml", GOOD.replace('"pending"', "true"))
        self.assertTrue(any("status" in p for p in validate.validate(self.dir).problems))
        self.write("example-window-opens.toml", GOOD.replace('id = "example-window-opens"', 'id = "example-window-opens\\n"'))
        self.assertTrue(any("id" in p for p in validate.validate(self.dir).problems))

    def test_a_byte_order_mark_or_a_duplicate_key_is_refused(self):
        (self.dir / "example-window-opens.toml").write_bytes(b"\xef\xbb\xbf" + GOOD.encode())
        self.assertEqual(len(validate.validate(self.dir).problems), 1)
        self.write("example-window-opens.toml", GOOD + 'title = "again"\n')
        self.assertEqual(len(validate.validate(self.dir).problems), 1)

    def test_a_subdirectory_with_scenarios_is_not_scanned_silently(self):
        self.write("example-window-opens.toml", GOOD)
        (self.dir / "nested").mkdir()
        (self.dir / "nested" / "x.toml").write_text(GOOD, encoding="utf-8")
        self.assertTrue(any("nested" in p for p in validate.validate(self.dir).problems))


class Main(Fixture):
    def run_main(self):
        lines = []
        code = validate.main(["--dir", str(self.dir)], lines.append)
        return code, lines

    def test_ok_line_counts_scenarios(self):
        self.write("example-window-opens.toml", GOOD)
        code, lines = self.run_main()
        self.assertEqual(code, 0)
        self.assertEqual(lines, ["OK 1 scenarios (0 active (unchecked: nothing runs the check), 1 pending), 0 violations"])

    def test_active_scenarios_are_reported_as_unchecked(self):
        self.write("example-window-opens.toml", ACTIVE)
        self.assertEqual(self.run_main()[1], ["OK 1 scenarios (1 active (unchecked: nothing runs the check), 0 pending), 0 violations"])

    def test_fewer_scenarios_than_the_floor_fail(self):
        self.write("example-window-opens.toml", GOOD)
        lines = []
        code = validate.main(["--dir", str(self.dir), "--min", "2"], lines.append)
        self.assertEqual(code, 1)
        self.assertTrue(any(line.startswith("VIOLATION") and "fewer than the floor of 2" in line for line in lines))

    def test_the_size_limit_is_64_kib(self):
        self.assertEqual(65536, validate.MAX_BYTES)

    def test_a_negative_floor_is_refused(self):
        with self.assertRaises(SystemExit):
            validate.main(["--dir", str(self.dir), "--min", "-1"], lambda line: None)

    def test_scenarios_at_the_floor_pass(self):
        self.write("example-window-opens.toml", GOOD)
        self.assertEqual(validate.main(["--dir", str(self.dir), "--min", "1"], lambda line: None), 0)

    def test_failure_lists_violations_then_a_fail_summary(self):
        self.write("example-window-opens.toml", GOOD.replace('"pending"', '"done"'))
        code, lines = self.run_main()
        self.assertEqual(code, 1)
        self.assertTrue(lines[0].startswith("VIOLATION "))
        self.assertTrue(lines[-1].startswith("FAIL "))


class RealScenarios(unittest.TestCase):
    def test_the_scenarios_in_the_repository_are_valid(self):
        found = validate.validate(Path(__file__).resolve().parent / "scenarios")
        self.assertEqual(found.problems, [])
        self.assertGreaterEqual(found.pending + found.active, 1)


if __name__ == "__main__":
    unittest.main()
