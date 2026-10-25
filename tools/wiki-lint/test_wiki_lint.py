import hashlib
import tempfile
import tomllib
import unittest
from pathlib import Path

import wiki_lint as wl

REPO = Path(__file__).resolve().parents[2]
GATES_TEXT = """
[coverage]
line_min = 90
new_line_min = 95
branch_min = 80
[clean_code]
function_length_warn = 60
"""
CONFIG = r"""
[scan]
paths = ["wiki"]

[history]
pages = ["wiki/old/*.md", "wiki/review.md"]

[[history_line]]
page = "wiki/README.md"
sha256 = "@V06@"

[[history_line]]
page = "wiki/README.md"
sha256 = "@V01@"

[[stale]]
pattern = '#include'
reason = "C++ leftover"

[[stale]]
pattern = '\bQ13\b'
reason = "resolved question"

[[threshold]]
key = "coverage.line_min"
number = 'Line coverage[^%]*?>= (\d+)%'

[[threshold]]
key = "coverage.new_line_min"
number = '(?:new|changed)(?:/changed)? lines >= (\d+)%'

[[threshold]]
key = "coverage.branch_min"
number = 'Branch[^%]*?>= (\d+)%'

[[threshold]]
key = "clean_code.function_length_warn"
number = 'warn > (\d+) lines'

[[known_number]]
page = "wiki/vision.md"
text = ">= 70%"
within = "efficiency"
reason = "a target, not a gate"
"""
V06 = "> - **v0.6** dropped #include"
V01 = "> - **v0.1** coverage >= 10%"
CONFIG = CONFIG.replace("@V06@", hashlib.sha256(V06.encode()).hexdigest()).replace("@V01@", hashlib.sha256(V01.encode()).hexdigest())
GATES = tomllib.loads(GATES_TEXT)


def lint(pages, config=CONFIG):
    return wl.check(pages, wl.load_config(config, GATES))


def rules(findings):
    return sorted({finding.rule for finding in findings})


class StaleTerms(unittest.TestCase):
    def test_a_clean_wiki_has_no_findings(self):
        self.assertEqual([], lint({"wiki/a.md": "# A\nNothing stale here.\n"}))

    def test_a_stale_term_is_reported_with_page_and_line(self):
        found = lint({"wiki/a.md": "ok\nuse #include <x>\n"})
        self.assertEqual([("stale-term", "wiki/a.md", 2)], [(f.rule, f.page, f.line) for f in found])

    def test_every_stale_pattern_is_checked(self):
        found = lint({"wiki/a.md": "#include\nsee Q13 here\n"})
        self.assertEqual([1, 2], [f.line for f in found])

    def test_a_history_page_is_exempt(self):
        self.assertEqual([], lint({"wiki/old/adr.md": "#include", "wiki/review.md": "Q13"}))

    def test_a_changelog_line_is_exempt_but_the_next_line_is_not(self):
        found = lint({"wiki/README.md": V06 + "\nbody #include\n"})
        self.assertEqual([2], [f.line for f in found])

    def test_a_changelog_line_is_exempt_only_on_its_own_page(self):
        self.assertEqual(["stale-term"], rules(lint({"wiki/other.md": V06 + "\n"})))

    def test_a_line_that_only_looks_like_a_changelog_entry_is_not_exempt(self):
        self.assertEqual(["stale-term"], rules(lint({"wiki/README.md": "> - v9 #include\n"})))

    def test_a_changelog_line_with_anything_added_is_no_longer_exempt(self):
        # review 0c-7 W1: the exemption used to cover the whole line once it began with a version tag
        for text in (V06 + " and Line coverage >= 50%", V06 + " #include", "> - **v9** Line coverage >= 50% and uses #include",
                     " " + V06 + " #include"):
            with self.subTest(text=text):
                self.assertNotEqual([], lint({"wiki/README.md": text + "\n"}))

    def test_a_changelog_line_ending_in_spaces_or_a_carriage_return_is_still_the_same_line(self):
        self.assertEqual([], lint({"wiki/README.md": V06 + "  \r\n"}))

    def test_a_history_line_must_name_a_sha256_not_a_pattern(self):
        for entry in ("pattern = '^> - v'", "sha256 = 'abc'", "sha256 = '" + "G" * 64 + "'", ""):
            with self.subTest(entry=entry), self.assertRaises(ValueError):
                wl.load_config(f"[[history_line]]\npage = 'wiki/README.md'\n{entry}\n", GATES)

    def test_a_struck_through_span_is_exempt_but_the_rest_of_the_row_is_not(self):
        self.assertEqual([], lint({"wiki/q.md": "| ~~**Q13**~~ resolved |\n"}))
        found = lint({"wiki/q.md": "| ~~x~~ | Line coverage >= 50% #include |\n"})
        self.assertEqual(["stale-term", "threshold-mismatch"], rules(found))

    def test_a_history_glob_does_not_reach_into_a_subfolder_or_another_extension(self):
        self.assertEqual(["stale-term"], rules(lint({"wiki/old/sub/x.md": "#include"})))
        self.assertEqual(["stale-term"], rules(lint({"wiki/old/x.txt": "#include"})))

    def test_case_spacing_and_hidden_character_variants_of_a_term_are_caught(self):
        config = CONFIG + "\n[[stale]]\npattern = '(?i)\\bcmake\\b'\nreason = 'toolchain'\n"
        config = config.replace(r"'\bQ13\b'", r"'\bQ\s?13\b'")
        for text in ("use cmake", "use CMake", "Q 13 answered", "Q1​3 answered"):
            with self.subTest(text=text):
                self.assertEqual(["stale-term"], rules(lint({"wiki/a.md": text}, config)))

    def test_a_term_only_matches_whole_words_when_the_pattern_says_so(self):
        self.assertEqual([], lint({"wiki/a.md": "Q130 and XQ13\n"}))


class HiddenText(unittest.TestCase):
    """Review 0c-7 W2: ways to write a term or a number that the rules used to miss."""

    def test_every_invisible_format_character_is_dropped_before_matching(self):
        for mark in ("­", "‎", "‏", "‪", "‮", "⁦", "⁠", "﻿", "​", "؜", "᠎"):
            with self.subTest(mark=hex(ord(mark))):
                self.assertEqual(["threshold-mismatch"], rules(lint({"wiki/a.md": f"Line cov{mark}erage >= 85%"})))
                self.assertEqual(["stale-term"], rules(lint({"wiki/a.md": f"#inc{mark}lude"})))

    def test_combining_marks_variation_selectors_and_blank_letters_are_dropped(self):
        for mark in ("͏", "️", "ㅤ", "⠀", "ᅟ"):
            with self.subTest(mark=hex(ord(mark))):
                self.assertEqual(["stale-term"], rules(lint({"wiki/a.md": f"#inc{mark}lude"})))

    def test_more_bound_wordings_are_refused_when_no_rule_covers_them(self):
        for text in ("greater than 80%", "exceeds 80%", "80% or higher", "80% minimum", "min 80%", "warn: 60"):
            with self.subTest(text=text):
                self.assertEqual(["threshold-uncovered"], rules(lint({"wiki/a.md": text})))

    def test_html_entities_are_decoded_before_matching(self):
        for text in ("Line coverage &ge; 85%", "Line coverage &gt;= 85%", "Line coverage &#62;= 85%", "Line coverage &#x3E;= 85%",
                     "Line coverage &GreaterEqual; 85%", "Line coverage &gt;&equals; 85%", "Line coverage >= 85&percnt;", "&num;include"):
            with self.subTest(text=text):
                self.assertNotEqual([], lint({"wiki/a.md": text}))

    def test_a_cyrillic_look_alike_inside_a_latin_word_is_a_finding(self):
        for text in ("Lіne coverage >= 50%", "uses cmаke", "Q1З"):
            with self.subTest(text=text):
                self.assertIn("mixed-script", rules(lint({"wiki/a.md": text})))

    def test_plain_cyrillic_and_greek_text_is_not_a_finding(self):
        self.assertEqual([], lint({"wiki/a.md": "Привет мир, Δt = 0.01 and α blending\n"}))

    def test_bounds_written_without_a_comparator_are_refused_when_no_rule_covers_them(self):
        for text in ("must reach 50 %", "Minimum line coverage: 50%", "coverage must reach 85 percent", "Maximum duplication: 9%",
                     "threshold of 50%", "floor: 80%"):
            with self.subTest(text=text):
                self.assertEqual(["threshold-uncovered"], rules(lint({"wiki/a.md": text})))

    def test_plain_percentages_are_still_not_thresholds(self):
        self.assertEqual([], lint({"wiki/a.md": "About 90% of the time, a 5% sample, 100% of cases\n"}))


class Thresholds(unittest.TestCase):
    def test_numbers_that_match_the_gate_configuration_pass(self):
        text = "Line coverage L0-L2 >= 90% (new lines >= 95%)\nfunction warn > 60 lines\n"
        self.assertEqual([], lint({"wiki/a.md": text}))

    def test_a_wrong_number_is_a_mismatch(self):
        found = lint({"wiki/a.md": "| Line coverage | tool | L0-L2 >= 85% |\n"})
        self.assertEqual(["threshold-mismatch"], rules(found))
        self.assertIn("coverage.line_min", found[0].message)
        self.assertIn("85", found[0].message)

    def test_the_new_line_number_is_checked_separately_from_the_line_number(self):
        found = lint({"wiki/a.md": "Line coverage >= 90%; new/changed lines >= 90%\n"})
        self.assertEqual(["threshold-mismatch"], rules(found))
        self.assertIn("coverage.new_line_min", found[0].message)

    def test_a_comparator_percentage_no_rule_covers_is_reported(self):
        found = lint({"wiki/a.md": "Mutation score >= 80%\n"})
        self.assertEqual(["threshold-uncovered"], rules(found))

    def test_a_warn_or_fail_number_no_rule_covers_is_reported(self):
        self.assertEqual(["threshold-uncovered"], rules(lint({"wiki/a.md": "fail > 15 is a failure\n"})))

    def test_a_known_number_is_allowed_on_its_page_only(self):
        self.assertEqual([], lint({"wiki/vision.md": "efficiency >= 70% on N cores\n"}))
        self.assertEqual(["threshold-uncovered"], rules(lint({"wiki/other.md": "efficiency >= 70% on N cores\n"})))

    def test_a_known_number_is_allowed_on_the_named_line_only(self):
        # review 0c-7 W3: it used to be allowed anywhere on its page
        self.assertEqual(["threshold-uncovered"], rules(lint({"wiki/vision.md": "Mutation score >= 70% is enough\n"})))
        self.assertEqual([], lint({"wiki/vision.md": "Mutation score\nefficiency >= 70% on N cores\n"}))

    def test_history_lines_and_pages_are_exempt_from_threshold_checks(self):
        pages = {"wiki/old/x.md": "Line coverage >= 10%", "wiki/README.md": "> - **v0.1** coverage >= 10%\n"}
        self.assertEqual([], lint(pages))

    def test_a_covered_number_is_not_also_reported_as_uncovered(self):
        found = lint({"wiki/a.md": "Line coverage >= 85%\n"})
        self.assertEqual(["threshold-mismatch"], rules(found))

    def test_the_page_may_state_the_number_twice_on_a_line(self):
        found = lint({"wiki/a.md": "Line coverage >= 90% and later Line coverage >= 91%\n"})
        self.assertEqual(1, len(found))

    def test_a_covered_number_does_not_hide_an_uncovered_one_on_the_same_line(self):
        for text in ("Line coverage >= 90% and mutation >= 50%", "mutation >= 50% but Line coverage >= 90%"):
            with self.subTest(text=text):
                self.assertEqual(["threshold-uncovered"], rules(lint({"wiki/a.md": text})))

    def test_a_second_bound_after_a_covered_one_is_not_covered(self):
        self.assertEqual(["threshold-uncovered"], rules(lint({"wiki/a.md": "Line coverage >= 90% >= 50%\n"})))

    def test_spellings_of_a_number_are_normalised_before_matching(self):
        wrong = ("Line coverage >= **85%**", "Line coverage ≥ 85%", "Line coverage ≥ 85 %", "Line coverage >=85%",
                 "Line coverage ＞＝ ８５％", "Line coverage >= 85 percent", "`Line coverage` >= 85%")
        for text in wrong:
            with self.subTest(text=text):
                self.assertEqual(["threshold-mismatch"], rules(lint({"wiki/a.md": text})))
        self.assertEqual([], lint({"wiki/a.md": "Line coverage ≥ 90%"}))
        self.assertEqual([], lint({"wiki/a.md": "**Line coverage** >= **90%**"}))

    def test_other_ways_to_write_a_bound_are_refused_when_no_rule_covers_them(self):
        for text in ("warn at 99 lines", "fail at 15", "Duplication under 9%", "at least 85 percent", "functions longer than 99 lines",
                     "coverage above 90.5%", "warn < 5", "fail >= 15", "coverage <= 9%"):
            with self.subTest(text=text):
                self.assertEqual(["threshold-uncovered"], rules(lint({"wiki/a.md": text})))

    def test_ordinary_numbers_with_over_or_under_are_not_thresholds(self):
        self.assertEqual([], lint({"wiki/a.md": "over 3 years, under 20 crates, at least 2 adapters\n"}))

    def test_a_number_without_a_comparator_is_not_a_threshold(self):
        self.assertEqual([], lint({"wiki/a.md": "About 90% of the time, 15 crates, version 0.6\n"}))


class Config(unittest.TestCase):
    def test_an_unknown_gate_key_is_an_error(self):
        bad = CONFIG.replace("coverage.line_min", "coverage.nope")
        with self.assertRaisesRegex(ValueError, "coverage.nope"):
            wl.load_config(bad, GATES)

    def test_a_bad_regex_is_an_error(self):
        with self.assertRaisesRegex(ValueError, "bad regex"):
            wl.load_config("[[stale]]\npattern = '('\nreason = 'x'\n", GATES)

    def test_a_number_regex_needs_exactly_one_group(self):
        for pattern in (r"no group", r"(\d+) and (\d+)"):
            with self.subTest(pattern=pattern):
                with self.assertRaisesRegex(ValueError, "one group"):
                    wl.load_config(f"[[threshold]]\nkey = 'coverage.line_min'\nnumber = '{pattern}'\n", GATES)

    def test_a_stale_entry_needs_a_reason(self):
        with self.assertRaisesRegex(ValueError, "reason"):
            wl.load_config("[[stale]]\npattern = 'x'\n", GATES)

    def test_a_known_number_needs_page_text_and_reason(self):
        with self.assertRaisesRegex(ValueError, "known_number"):
            wl.load_config("[[known_number]]\npage = 'wiki/a.md'\n", GATES)

    def test_a_known_number_needs_the_line_it_belongs_to(self):
        with self.assertRaisesRegex(ValueError, "within"):
            wl.load_config("[[known_number]]\npage = 'wiki/a.md'\ntext = '>= 7%'\nreason = 'r'\n", GATES)

    def test_a_gate_key_without_any_rule_is_an_error(self):
        # a gate number that no wiki rule watches would be silently unchecked
        with self.assertRaisesRegex(ValueError, "score_min"):
            wl.load_config(CONFIG, {**GATES, "mutation": {"score_min": 80}}, require_all_keys=True)


class Main(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: __import__("shutil").rmtree(self.dir, ignore_errors=True))
        (self.dir / "wiki").mkdir()
        (self.dir / "cfg.toml").write_text(CONFIG, encoding="utf-8")
        (self.dir / "gates.toml").write_text(GATES_TEXT, encoding="utf-8")

    def run_main(self, *extra):
        lines = []
        code = wl.main(["--config", str(self.dir / "cfg.toml"), "--gates", str(self.dir / "gates.toml"), "--repo", str(self.dir), *extra],
                       print_line=lines.append)
        return code, lines

    def test_an_empty_wiki_is_a_failure_not_a_pass(self):
        code, lines = self.run_main()
        self.assertEqual(1, code)
        self.assertIn("no pages", lines[0])

    def test_files_and_folders_in_scan_paths_are_both_read(self):
        (self.dir / "CLAUDE.md").write_text("#include\n", encoding="utf-8")
        (self.dir / "wiki" / "a.md").write_text("fine\n", encoding="utf-8")
        (self.dir / "cfg.toml").write_text(CONFIG.replace('paths = ["wiki"]', 'paths = ["wiki", "CLAUDE.md"]'), encoding="utf-8")
        code, lines = self.run_main()
        self.assertIn("CLAUDE.md:1", lines[0])

    def test_a_missing_scan_path_is_a_failure(self):
        (self.dir / "wiki" / "a.md").write_text("fine\n", encoding="utf-8")
        (self.dir / "cfg.toml").write_text(CONFIG.replace('paths = ["wiki"]', 'paths = ["wiki", "NOPE.md"]'), encoding="utf-8")
        code, lines = self.run_main()
        self.assertEqual(1, code)
        self.assertIn("NOPE.md", lines[0])

    def test_a_clean_wiki_exits_zero(self):
        (self.dir / "wiki" / "a.md").write_text("fine\n", encoding="utf-8")
        code, lines = self.run_main()
        self.assertEqual(0, code)
        self.assertEqual("OK 1 pages checked, 0 violations", lines[-1])

    def test_a_seeded_violation_exits_one_with_a_readable_line(self):
        (self.dir / "wiki" / "a.md").write_text("fine\nuse #include\n", encoding="utf-8")
        code, lines = self.run_main()
        self.assertEqual(1, code)
        self.assertEqual("VIOLATION stale-term wiki/a.md:2 C++ leftover", lines[0])
        self.assertEqual("FAIL 1 pages checked, 1 violations", lines[-1])

    def test_pages_in_subfolders_are_found_with_forward_slashes(self):
        (self.dir / "wiki" / "sub").mkdir()
        (self.dir / "wiki" / "sub" / "b.md").write_text("#include\n", encoding="utf-8")
        code, lines = self.run_main()
        self.assertIn("wiki/sub/b.md:1", lines[0])

    def test_a_missing_wiki_folder_is_a_failure_not_a_pass(self):
        (self.dir / "wiki").rmdir()
        code, lines = self.run_main()
        self.assertEqual(1, code)
        self.assertTrue(lines[0].startswith("FAIL "))

    def test_a_bad_config_is_a_failure(self):
        (self.dir / "cfg.toml").write_text("[[stale]]\npattern = '('\nreason='x'\n", encoding="utf-8")
        code, lines = self.run_main()
        self.assertEqual(1, code)
        self.assertIn("bad regex", lines[0])


class RealWiki(unittest.TestCase):
    def test_the_real_wiki_passes_the_real_configuration(self):
        lines = []
        code = wl.main(["--config", str(REPO / "gates" / "wiki-lint.toml"), "--gates", str(REPO / "gates" / "gates.toml"),
                        "--repo", str(REPO)], print_line=lines.append)
        self.assertEqual((0, []), (code, [line for line in lines if line.startswith(("VIOLATION", "FAIL"))]))

    def test_a_drifted_number_in_a_real_page_is_caught_with_the_real_configuration(self):
        config = wl.load_config((REPO / "gates" / "wiki-lint.toml").read_text(encoding="utf-8"),
                                tomllib.loads((REPO / "gates" / "gates.toml").read_text(encoding="utf-8")))
        page = "wiki/workflow/quality-metrics.md"
        text = (REPO / page).read_text(encoding="utf-8")
        self.assertIn("L0-L2 >= 90%", text)
        found = wl.check({page: text.replace("L0-L2 >= 90%", "L0-L2 >= 85%")}, config)
        self.assertEqual(["threshold-mismatch"], sorted({finding.rule for finding in found}))

    def test_a_stale_term_added_to_a_real_changelog_line_is_caught(self):
        config = wl.load_config((REPO / "gates" / "wiki-lint.toml").read_text(encoding="utf-8"),
                                tomllib.loads((REPO / "gates" / "gates.toml").read_text(encoding="utf-8")))
        text = (REPO / "wiki" / "README.md").read_text(encoding="utf-8")
        line = next(line for line in text.split("\n") if line.startswith("> - **v0.6**"))
        self.assertEqual([], wl.check({"wiki/README.md": text}, config))
        found = wl.check({"wiki/README.md": text.replace(line, line + " Line coverage >= 50%")}, config)
        self.assertIn("threshold-mismatch", {finding.rule for finding in found})

    def test_every_history_line_hash_matches_a_real_line(self):
        data = tomllib.loads((REPO / "gates" / "wiki-lint.toml").read_text(encoding="utf-8"))
        for entry in data["history_line"]:
            lines = (REPO / entry["page"]).read_text(encoding="utf-8").splitlines()
            self.assertIn(entry["sha256"], {hashlib.sha256(line.rstrip().encode()).hexdigest() for line in lines})

    def test_the_history_pages_exist_and_the_review_page_is_protected(self):
        config = tomllib.loads((REPO / "gates" / "wiki-lint.toml").read_text(encoding="utf-8"))
        protected = (REPO / "gates" / "protected-paths.txt").read_text(encoding="utf-8").split()
        for page in config["history"]["pages"]:
            with self.subTest(page=page):
                if "*" not in page:
                    self.assertTrue((REPO / page).is_file(), "a history exemption for a page that does not exist")
                    self.assertIn(page, protected)
                else:
                    self.assertTrue(any(page.startswith(entry) for entry in protected if entry.endswith("/")))

    def test_every_gate_number_has_a_wiki_rule(self):
        config = (REPO / "gates" / "wiki-lint.toml").read_text(encoding="utf-8")
        gates = tomllib.loads((REPO / "gates" / "gates.toml").read_text(encoding="utf-8"))
        wl.load_config(config, gates, require_all_keys=True)


if __name__ == "__main__":
    unittest.main()
