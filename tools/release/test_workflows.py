"""Static checks of the release and PR-title workflows (card 0b-6).

The workflows cannot run in a unit test, so these tests pin the properties that
matter for safety and for "two merges cannot compute the same version":
trigger, serialisation, token scope, pinned actions, no expression injection.
"""

import re
import unittest
from pathlib import Path

WORKFLOWS = Path(__file__).resolve().parents[2] / ".github" / "workflows"
RELEASE = (WORKFLOWS / "release.yml").read_text(encoding="utf-8")
PR_TITLE = (WORKFLOWS / "pr-title.yml").read_text(encoding="utf-8")
CI = (WORKFLOWS / "ci.yml").read_text(encoding="utf-8")
GUARD = (WORKFLOWS / "governance-guard.yml").read_text(encoding="utf-8")

PINNED = re.compile(r"^\s*-?\s*uses:\s*\S+@[0-9a-f]{40}\b", re.M)
USES = re.compile(r"^\s*-?\s*uses:\s*(\S+)", re.M)


def run_blocks(text):
    """The shell text of each `run:` step, so expressions inside them can be found."""
    blocks, lines, i = [], text.split("\n"), 0
    while i < len(lines):
        match = re.match(r"^(\s*)(?:- )?run:\s*(.*)$", lines[i])
        if not match:
            i += 1
            continue
        indent, first = len(match.group(1)), match.group(2)
        body = [first] if first and first not in ("|", ">", "|-", ">-") else []
        i += 1
        while i < len(lines) and (not lines[i].strip() or len(lines[i]) - len(lines[i].lstrip()) > indent):
            body.append(lines[i])
            i += 1
        blocks.append("\n".join(body))
    return blocks


class Release(unittest.TestCase):
    def test_runs_only_on_pushes_to_main(self):
        self.assertRegex(RELEASE, r"(?m)^on:\n  push:\n    branches: \[main\]\n")
        self.assertNotIn("pull_request", RELEASE)
        self.assertNotIn("workflow_dispatch", RELEASE)

    def test_releases_are_serialised_and_never_cancelled(self):
        self.assertRegex(RELEASE, r"(?m)^concurrency:\n  group: release\n  cancel-in-progress: false\n")

    def test_token_can_write_contents_and_read_pull_requests_and_nothing_else(self):
        # pull-requests: read is only for the squash-subject check (card 0b-8).
        self.assertRegex(RELEASE, r"(?m)^permissions:\n  contents: write\n  pull-requests: read\n(?!  )")

    def test_squash_subject_is_checked_before_anything_is_built_or_published(self):
        check = RELEASE.find("check_squash_subject.py")
        self.assertGreater(check, -1)
        self.assertLess(check, RELEASE.find("make_assets.py"))
        self.assertLess(check, RELEASE.find("gh release create"))
        self.assertIn("steps.plan.outputs.skip != 'true'", RELEASE[RELEASE.rfind("- name", 0, check):check])

    def test_subject_and_title_reach_the_script_through_the_shell_not_an_expression(self):
        blocks = [b for b in run_blocks(RELEASE) if "check_squash_subject.py" in b]
        self.assertTrue(blocks)
        for block in blocks:
            self.assertIn("git log -1 --format=%s", block)
            self.assertIn('--subject="$subject"', block)
            self.assertIn('gh api "repos/$GITHUB_REPOSITORY/pulls/', block)

    def test_uses_no_secrets(self):
        self.assertNotIn("secrets.", RELEASE)

    def test_every_action_is_pinned_by_commit(self):
        uses = USES.findall(RELEASE)
        self.assertTrue(uses)
        self.assertEqual(len(PINNED.findall(RELEASE)), len(uses))

    def test_checkout_has_full_history_and_keeps_no_credentials(self):
        self.assertIn("fetch-depth: 0", RELEASE)
        self.assertIn("persist-credentials: false", RELEASE)

    def test_release_is_published_only_after_assets_are_uploaded(self):
        create, publish = RELEASE.find("--draft"), RELEASE.find("draft=false")
        self.assertGreater(create, -1)
        self.assertGreater(publish, create)

    def test_tag_is_created_at_the_merge_commit(self):
        self.assertIn('--target "$GITHUB_SHA"', RELEASE)

    def test_skips_a_commit_that_already_has_a_release(self):
        self.assertIn("steps.plan.outputs.skip", RELEASE)

    def test_never_pushes_to_a_branch(self):
        for block in run_blocks(RELEASE):
            self.assertNotRegex(block, r"git push")

    def test_no_expression_is_expanded_inside_a_shell_script(self):
        for block in run_blocks(RELEASE):
            self.assertNotIn("${{", block)


class PullRequestTitle(unittest.TestCase):
    def test_runs_on_pull_request_never_pull_request_target(self):
        self.assertIn("pull_request:", PR_TITLE)
        self.assertNotIn("pull_request_target", PR_TITLE)

    def test_title_is_checked_again_after_an_edit(self):
        self.assertIn("edited", PR_TITLE)

    def test_checks_out_the_base_commit_not_the_pr_tree(self):
        self.assertIn("ref: ${{ github.event.pull_request.base.sha }}", PR_TITLE)

    def test_title_reaches_the_script_through_the_environment(self):
        self.assertIn("PR_TITLE: ${{ github.event.pull_request.title }}", PR_TITLE)
        for block in run_blocks(PR_TITLE):
            self.assertNotIn("${{", block)

    def test_token_is_read_only_and_actions_are_pinned(self):
        self.assertRegex(PR_TITLE, r"(?m)^permissions:\n  contents: read\n(?!  )")
        self.assertEqual(len(PINNED.findall(PR_TITLE)), len(USES.findall(PR_TITLE)))


class GovernanceGuard(unittest.TestCase):
    def test_runs_on_pull_request_never_pull_request_target(self):
        self.assertIn("pull_request:", GUARD)
        self.assertNotIn("pull_request_target", GUARD)

    def test_runs_again_after_a_description_edit(self):
        self.assertIn("edited", GUARD)

    def test_the_tool_runs_from_the_base_commit_and_the_pr_tree_is_only_data(self):
        self.assertIn("ref: ${{ github.event.pull_request.base.sha }}", GUARD)
        self.assertIn("ref: ${{ github.event.pull_request.head.sha }}", GUARD)
        self.assertIn("python base/tools/gate-check/governance_guard.py", GUARD)
        self.assertIn("--paths base/gates/protected-paths.txt", GUARD)

    def test_description_reaches_the_script_through_the_environment(self):
        self.assertIn("PR_BODY: ${{ github.event.pull_request.body }}", GUARD)
        for block in run_blocks(GUARD):
            self.assertNotIn("${{", block)

    def test_token_is_read_only_and_actions_are_pinned(self):
        self.assertRegex(GUARD, r"(?m)^permissions:\n  contents: read\n(?!  )")
        self.assertTrue(USES.findall(GUARD))
        self.assertEqual(len(PINNED.findall(GUARD)), len(USES.findall(GUARD)))

    def test_checkouts_keep_no_credentials(self):
        self.assertEqual(GUARD.count("persist-credentials: false"), GUARD.count("actions/checkout@"))

    def test_a_missing_tool_on_the_base_is_a_notice_not_a_failure(self):
        self.assertIn("::notice::", GUARD)


class Ci(unittest.TestCase):
    def test_ci_runs_on_main_too_and_does_not_cancel_runs_on_main(self):
        self.assertNotIn("branches-ignore", CI)
        self.assertIn("cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}", CI)

    def test_the_report_only_metrics_job_skips_main_and_tags_to_save_minutes(self):
        job = CI[CI.index("\n  metrics:"):CI.index("\n  deny:")]
        self.assertIn("if: github.ref_type == 'branch' && github.ref != 'refs/heads/main'", job)

    def test_ci_runs_the_tests_of_every_tool_folder(self):
        for folder, module in (("tools/templates", "test_templates"), ("tools/token-probe", "test_probe"),
                               ("tools/gate-check", "test_governance_guard"),
                               ("tools/classify-change", "test_classify_change"),
                               ("tools/arch-check", "test_arch_check")):
            with self.subTest(folder=folder):
                self.assertIn(f"working-directory: {folder}", CI)
                self.assertIn(module, CI)

    def test_ci_runs_the_architecture_check_on_the_real_workspace(self):
        self.assertIn("python tools/arch-check/arch_check.py --config gates/architecture.toml", CI)

    def test_ci_runs_the_release_tool_tests(self):
        self.assertIn("working-directory: tools/release", CI)
        for module in ("test_next_version", "test_release_plan", "test_check_pr_title",
                       "test_make_assets", "test_workflows", "test_check_squash_subject"):
            with self.subTest(module=module):
                self.assertIn(module, CI)


if __name__ == "__main__":
    unittest.main()
