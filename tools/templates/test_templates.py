"""Structure tests for the task-card and review-report templates.

The required sections come from wiki/workflow/tdd-workflow.md section 10 (task
cards), wiki/workflow/clean-code-and-review.md section 4.3 (review report) and
wiki/workflow/delivery-pipeline.md (acceptance basis, governance flag,
classifier verdict, loop count). Every check is scoped to the body of its own
section, so prose elsewhere in the file cannot satisfy it.
"""

import re
import unittest
from pathlib import Path

HERE = Path(__file__).parent

TASK_CARD_SECTIONS = (
    "Module and layer",
    "Class",
    "Governance task",
    "Behaviour",
    "Tests to write first",
    "Performance expectation",
    "Files allowed to change",
    "Acceptance basis",
    "Done",
)

REVIEW_REPORT_SECTIONS = (
    "Summary verdict",
    "Checklist",
    "Findings",
    "Not verified",
    "Metrics snapshot",
)

REVIEW_REPORT_HEADER_FIELDS = (
    "Task",
    "SHA reviewed",
    "Class",
    "Classifier verdict",
    "Loops",
    "Author",
    "Reviewer",
)

REVIEW_CHECKLIST_AREAS = (
    "Measurable rules",
    "TDD",
    "Architecture",
    "Security",
    "Performance",
    "Product fit",
)

ACCEPTANCE_BASES = ("Product view", "Metrics", "Both")
SEVERITIES = ("blocker", "major", "minor")
FINDING_COLUMNS = ("id", "severity", "location", "description", "status")


def read(name):
    return (HERE / name).read_text(encoding="utf-8")


def sections(text):
    """Map each level-2 heading to its body (up to the next level-2 heading)."""
    parts = re.split(r"^## +(.+)$", text, flags=re.MULTILINE)
    return {parts[i].strip(): parts[i + 1] for i in range(1, len(parts) - 1, 2)}


class TaskCardTemplate(unittest.TestCase):
    def setUp(self):
        self.text = read("task-card.md")
        self.body = sections(self.text)

    def test_has_every_required_section(self):
        for section in TASK_CARD_SECTIONS:
            with self.subTest(section=section):
                self.assertIn(section, self.body)

    def test_title_line_names_the_task_id_pattern(self):
        self.assertRegex(self.text, r"^# <task-id> ")

    def test_class_section_lists_the_three_classes(self):
        for name in ("1 routine", "2 sensitive", "3 governance"):
            with self.subTest(name=name):
                self.assertIn(name, self.body["Class"])

    def test_governance_flag_is_yes_or_no(self):
        self.assertIn("yes | no", self.body["Governance task"])

    def test_acceptance_section_offers_each_basis_as_an_option(self):
        options = next(l for l in self.body["Acceptance basis"].splitlines() if l.strip())
        for basis in ACCEPTANCE_BASES:
            with self.subTest(basis=basis):
                self.assertIn(basis, options)

    def test_behaviour_is_given_when_then(self):
        for word in ("Given", "When", "Then"):
            with self.subTest(word=word):
                self.assertIn(word, self.body["Behaviour"])

    def test_done_means_gates_green(self):
        self.assertIn("gates green", self.body["Done"])


class ReviewReportTemplate(unittest.TestCase):
    def setUp(self):
        self.text = read("review-report.md")
        self.body = sections(self.text)

    def test_has_every_required_section(self):
        for section in REVIEW_REPORT_SECTIONS:
            with self.subTest(section=section):
                self.assertIn(section, self.body)

    def test_header_has_every_field(self):
        for field in REVIEW_REPORT_HEADER_FIELDS:
            with self.subTest(field=field):
                self.assertRegex(self.text, rf"(?m)^{re.escape(field)}: \S")

    def test_no_session_link_or_session_identifier_is_asked_for(self):
        # Card 4-26 (owner): the repository is public, so reports record roles, never session ids or URLs.
        self.assertNotRegex(self.text.lower(), r"session")

    def test_loops_field_asks_for_a_number(self):
        self.assertRegex(self.text, r"(?m)^Loops: <n>")

    def test_checklist_has_one_bullet_per_area(self):
        for area in REVIEW_CHECKLIST_AREAS:
            with self.subTest(area=area):
                self.assertRegex(self.body["Checklist"], rf"(?m)^- {re.escape(area)}:")

    def test_findings_table_has_the_agreed_columns(self):
        rows = [r for r in self.body["Findings"].splitlines() if r.startswith("|")]
        self.assertTrue(rows, "findings table is missing")
        cells = [c.strip().lower() for c in rows[0].strip("|").split("|")]
        self.assertEqual(cells, list(FINDING_COLUMNS))

    def test_findings_section_names_the_three_severities(self):
        for severity in SEVERITIES:
            with self.subTest(severity=severity):
                self.assertIn(severity, self.body["Findings"])

    def test_verdict_is_ready_or_not_ready(self):
        self.assertIn("ready | not ready", self.body["Summary verdict"])

    def test_ready_requires_zero_open_blocker_major(self):
        self.assertIn("0 open blocker/major", self.body["Summary verdict"])


if __name__ == "__main__":
    unittest.main()
