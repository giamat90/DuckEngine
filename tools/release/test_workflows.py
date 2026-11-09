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
TRUSTED = (WORKFLOWS / "trusted-pr-gates.yml").read_text(encoding="utf-8")

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

    def test_a_develop_merge_is_checked_for_its_footer_before_anything_is_built(self):
        footer = RELEASE.find("check_squash_subject.py footer")
        self.assertGreater(footer, -1)
        self.assertLess(footer, RELEASE.find("make_assets.py"))
        self.assertIn("head.ref", RELEASE)
        self.assertIn('--message="$(git log -1 --format=%B)"', RELEASE)
        self.assertIn("steps.plan.outputs.skip != 'true'", RELEASE[RELEASE.rfind("- name", 0, footer):footer])

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

    def test_checks_out_main_not_the_pr_tree_and_not_the_base_branch(self):
        self.assertRegex(PR_TITLE, r"(?m)^          ref: main$")
        self.assertNotIn("pull_request.base", PR_TITLE)
        self.assertNotIn("pull_request.head.sha", PR_TITLE)

    def test_a_develop_pr_is_checked_for_its_release_footer(self):
        self.assertIn("github.event.pull_request.head.ref == 'develop'", PR_TITLE)
        # A fork's branch named develop is not a release PR.
        self.assertIn("github.event.pull_request.head.repo.full_name == github.repository", PR_TITLE)
        self.assertIn("check_squash_subject.py footer --batch-only", PR_TITLE)
        self.assertIn("HEAD_REF: ${{ github.event.pull_request.head.ref }}", PR_TITLE)
        for block in run_blocks(PR_TITLE):
            self.assertNotIn("${{", block)

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

    def test_the_tool_runs_from_main_and_the_pr_tree_is_only_data(self):
        self.assertRegex(GUARD, r"(?m)^          ref: main\n          path: base$")
        self.assertNotIn("ref: ${{ github.event.pull_request.base.sha }}", GUARD)
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


class TrustedPrGates(unittest.TestCase):
    """trusted-pr-gates.yml runs from the base branch, so a PR cannot edit or delete it (card 0c-14)."""

    def test_runs_on_pull_request_target_so_the_base_version_always_runs(self):
        self.assertRegex(TRUSTED, r"(?m)^on:\n  pull_request_target:\n")
        self.assertNotRegex(TRUSTED, r"(?m)^  pull_request:")
        for event in ("opened", "edited", "reopened", "synchronize"):
            self.assertIn(event, TRUSTED)

    def test_only_pull_requests_into_main_are_served(self):
        # pull_request_target loads the workflow of the PR's base branch: without this filter a PR
        # into a branch someone has edited would run that branch's version of this file.
        self.assertIn("    branches: [main]\n", TRUSTED)

    def test_the_token_is_read_only_and_no_secret_is_used(self):
        self.assertRegex(TRUSTED, r"(?m)^permissions:\n  contents: read\n(?!  )")
        self.assertNotIn("secrets.", TRUSTED)

    def test_the_pr_tree_is_data_and_never_executed(self):
        for block in run_blocks(TRUSTED):
            for command in re.findall(r"(?m)^\s*(python3?\b[^\n]*)", block):
                self.assertRegex(command, r"^python base/")
            self.assertNotRegex(block, r"\b(python3?|node|pytest|pip)\b\s+(-\S+\s+)*(\./)?head/")
            self.assertNotRegex(block, r"(?m)^\s*(\./)?head/")
            self.assertNotRegex(block, r"\b(cargo|npm|make|bash|sh|source)\b[^\n]*head/")
            self.assertNotIn("cd head", block)
        self.assertNotIn("working-directory: head", TRUSTED)
        self.assertEqual(TRUSTED.count("path: head"), 1)

    def test_the_tools_and_lists_come_from_the_base_checkout(self):
        for side in ("base", "head"):
            self.assertRegex(TRUSTED, r"ref: \$\{\{ github.event.pull_request." + side + r".sha \}\}\n\s+path: " + side + r"\n")
        self.assertIn("fetch-depth: 0", TRUSTED)
        self.assertIn('--base "$BASE_SHA" --head "$HEAD_SHA"', TRUSTED)
        self.assertEqual(TRUSTED.count('--body-file "$RUNNER_TEMP/pr-body.md"'), 2)
        for name, side in (("BASE_SHA", "base"), ("HEAD_SHA", "head")):
            self.assertIn(name + ": ${{ github.event.pull_request." + side + ".sha }}", TRUSTED)
        self.assertIn("python base/tools/gate-check/governance_guard.py", TRUSTED)
        self.assertIn("--paths base/gates/protected-paths.txt", TRUSTED)
        self.assertIn("python base/tools/gate-check/pr_gates.py workflows --repo head "
                      "--required base/gates/required-workflows.txt", TRUSTED)
        self.assertIn("python base/tools/gate-check/pr_gates.py approval --repo head", TRUSTED)
        self.assertIn("python base/acceptance/validate.py --dir head/acceptance/scenarios --min 1", TRUSTED)

    def test_checkouts_keep_no_credentials_and_actions_are_pinned(self):
        self.assertEqual(TRUSTED.count("persist-credentials: false"), TRUSTED.count("actions/checkout@"))
        self.assertTrue(USES.findall(TRUSTED))
        self.assertEqual(len(PINNED.findall(TRUSTED)), len(USES.findall(TRUSTED)))

    def test_the_description_reaches_the_scripts_through_the_environment(self):
        self.assertIn("PR_BODY: ${{ github.event.pull_request.body }}", TRUSTED)
        for block in run_blocks(TRUSTED):
            self.assertNotIn("${{", block)

    def test_the_gate_steps_cannot_be_skipped_or_allowed_to_fail(self):
        self.assertNotIn("continue-on-error", TRUSTED)
        self.assertNotIn("if:", TRUSTED)

    def test_a_missing_tool_on_the_base_is_not_skipped_silently(self):
        # Unlike governance-guard.yml there is no bootstrap exit: the tools exist on the base now.
        self.assertNotIn("::notice::", TRUSTED)
        self.assertNotIn("exit 0", TRUSTED)


class Ci(unittest.TestCase):
    def test_ci_runs_only_on_main_and_on_demand_not_on_every_branch_push(self):
        # Develop-branch workflow (cards 1-38, 2-9): a branch push must not spend minutes; the agent starts
        # one run on the exact commit to approve (`gh workflow run ci.yml --ref <branch>`).
        self.assertRegex(CI, r"(?m)^on:\n  push:\n    branches: \[main\]\n  workflow_dispatch:\n")
        self.assertNotIn("pull_request", CI.split("\npermissions:")[0])

    def test_ci_runs_on_main_too_and_does_not_cancel_runs_on_main(self):
        self.assertNotIn("branches-ignore", CI)
        self.assertIn("cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}", CI)

    def test_the_report_only_metrics_job_skips_main_and_tags_to_save_minutes(self):
        job = CI[CI.index("\n  metrics:"):CI.index("\n  deny:")]
        self.assertIn("if: github.ref_type == 'branch' && github.ref != 'refs/heads/main'", job)

    def test_cargo_deny_runs_once_on_linux_because_the_graph_names_both_targets(self):
        # deny.toml [graph].targets lists the Windows and the Linux triple, so one Linux leg checks the same graph
        # as two legs would (card 4-28); the Windows leg only cost wall clock.
        deny = (WORKFLOWS.parent.parent / "deny.toml").read_text(encoding="utf-8")
        self.assertIn("x86_64-pc-windows-msvc", deny)
        self.assertIn("x86_64-unknown-linux-gnu", deny)
        job = re.split(r"\n  [a-z_0-9]+:\n", CI[CI.index("\n  deny:") + 1:], maxsplit=1)[0]
        self.assertIn("ubuntu-latest", job)
        self.assertNotIn("windows-latest", job)

    def test_ci_lints_and_tests_base_with_the_profiling_feature_on_in_the_build_job(self):
        # The profiling hook compiles to nothing with the feature off, so the default run never sees it (card 1-41).
        build = CI[:CI.index("\n  gates:")]
        lint = "cargo clippy -p base --features profiling --all-targets --locked -- -D warnings"
        test = "cargo test -p base --features profiling --locked"
        self.assertIn(lint, build)
        self.assertIn(test, build)
        self.assertLess(build.index("cargo test --workspace --locked"), build.index(test))

    def test_the_headless_job_builds_and_tests_without_any_adapter_or_application_crate(self):
        # Phase 2 exit criterion (card 2-8): the engine must build and pass its tests with no real OS, GPU
        # or audio adapter present; excluded crates must also be absent from the dependency graph.
        job = CI[CI.index("\n  headless:") + 1:]
        following = re.search(r"\n  \w+:\n", job)  # the next job key, if a job is ever added after this one
        job = job[:following.start()] if following else job
        self.assertIn("runs-on: ubuntu-latest", job)
        self.assertNotIn("matrix", job)
        self.assertIn("--exclude 'adapter_*' --exclude 'app_*'", job)
        self.assertIn("cargo test --workspace --exclude 'adapter_*' --exclude 'app_*' --locked", job)
        # Every target and every feature, not only the runner's: a Windows-only or optional dependency counts.
        self.assertIn("cargo tree --workspace --exclude 'adapter_*' --exclude 'app_*' --locked --target all "
                      "--all-features --edges normal,build,dev", job)
        self.assertEqual(job.count("persist-credentials: false"), job.count("actions/checkout@"))
        self.assertNotIn("continue-on-error", job)
        self.assertNotRegex(job, r"(?m)^\s+if:")

    def test_the_glfw_linux_job_runs_the_real_adapter_under_a_virtual_display(self):
        # Phase 2 exit criterion (card 2-11): the window contract on Linux with the real GLFW library.
        job = CI[CI.index("\n  glfw_linux:") + 1:]
        following = re.search(r"\n  \w+:\n", job)
        job = job[:following.start()] if following else job
        self.assertIn("runs-on: ubuntu-latest", job)
        self.assertNotIn("matrix", job)
        self.assertIn("xvfb libglfw3", job)
        self.assertIn('DUCK_REQUIRE_GLFW: "1"', job)
        self.assertIn("xvfb-run --auto-servernum cargo test -p adapter_window_glfw --locked", job)
        self.assertEqual(job.count("persist-credentials: false"), job.count("actions/checkout@"))
        self.assertNotIn("continue-on-error", job)

    def test_the_glfw_linux_job_says_so_when_the_adapter_is_not_in_the_workspace_yet(self):
        # `main` may be behind `develop`: a missing crate is reported, never silently green.
        job = CI[CI.index("\n  glfw_linux:") + 1:]
        following = re.search(r"\n  \w+:\n", job)
        job = job[:following.start()] if following else job
        # Presence is decided from the tree; once the crate folder exists a cargo failure (a broken manifest)
        # fails the job instead of passing as "absent".
        self.assertIn("if [ -d engine/adapter_window_glfw ]; then", job)
        self.assertIn("cargo pkgid -p adapter_window_glfw > /dev/null", job)
        self.assertNotIn("pkgid -p adapter_window_glfw > /dev/null 2>&1", job)
        self.assertIn("::notice::not run: adapter_window_glfw is not in this workspace yet", job)
        self.assertIn('>> "$GITHUB_STEP_SUMMARY"', job)
        self.assertEqual(job.count("if: steps.adapter.outputs.present == 'true'"), 2)

    def test_the_gl_linux_job_runs_the_gl_adapter_on_llvmpipe_and_reports_the_version(self):
        # Card 3-12 (O7): the GL adapter and the render contract on Linux; a device is required, not optional.
        job = CI[CI.index("\n  gl_linux:") + 1:]
        following = re.search(r"\n  \w+:\n", job)
        job = job[:following.start()] if following else job
        self.assertIn("runs-on: ubuntu-latest", job)
        self.assertNotIn("matrix", job)
        self.assertIn("xvfb libglfw3 mesa-utils libgl1-mesa-dri", job)
        self.assertIn('LIBGL_ALWAYS_SOFTWARE: "1"', job)
        self.assertIn('DUCK_REQUIRE_GL: "1"', job)
        self.assertIn("glxinfo -B", job)
        self.assertIn("xvfb-run --auto-servernum cargo test -p adapter_render_gl --locked", job)
        self.assertEqual(job.count("persist-credentials: false"), job.count("actions/checkout@"))
        self.assertNotIn("continue-on-error", job)

    def test_the_gl_linux_job_says_so_when_the_adapter_is_not_in_the_workspace_yet(self):
        # `main` may be behind `develop`: a missing crate is reported, never silently green.
        job = CI[CI.index("\n  gl_linux:") + 1:]
        following = re.search(r"\n  \w+:\n", job)
        job = job[:following.start()] if following else job
        self.assertIn("if [ -d engine/adapter_render_gl ]; then", job)
        self.assertIn("cargo pkgid -p adapter_render_gl > /dev/null", job)
        self.assertNotIn("pkgid -p adapter_render_gl > /dev/null 2>&1", job)
        self.assertIn("::notice::not run: adapter_render_gl is not in this workspace yet", job)
        self.assertIn('>> "$GITHUB_STEP_SUMMARY"', job)
        self.assertEqual(job.count("if: steps.adapter.outputs.present == 'true'"), 3)

    def test_the_arm64_job_runs_on_a_hosted_arm_runner_and_proves_the_cpu(self):
        # Card 2-10: a hosted ARM64 runner exists for this repository (probed while it was private, 2026-11-01), so the NEON
        # backend runs on real AArch64 hardware, not under an emulator.
        job = CI[CI.index("\n  arm64:") + 1:]
        following = re.search(r"\n  \w+:\n", job)
        job = job[:following.start()] if following else job
        self.assertIn("runs-on: ubuntu-24.04-arm", job)
        self.assertNotIn("matrix", job)
        self.assertIn('test "$(uname -m)" = aarch64', job)
        self.assertIn("cargo clippy --workspace --exclude 'adapter_*' --all-targets --locked -- -D warnings", job)
        self.assertIn("cargo test --workspace --exclude 'adapter_*' --locked", job)
        self.assertEqual(job.count("persist-credentials: false"), job.count("actions/checkout@"))
        self.assertNotIn("continue-on-error", job)
        self.assertNotRegex(job, r"(?m)^\s+if:")
        self.assertNotIn("qemu", job)

    def d2_job(self):
        job = CI[CI.index("\n  d2_determinism:") + 1:]
        following = re.search(r"\n  \w+:\n", job)
        return job[:following.start()] if following else job

    def test_the_d2_job_runs_on_windows_and_linux_with_the_feature_on_and_a_trusted_golden_file(self):
        # Card 4-20: the golden file and the checker come from main; the pushed commit is the code under test.
        job = self.d2_job()
        self.assertIn("os: [windows-latest, ubuntu-latest]", job)
        self.assertIn("fail-fast: false", job)
        self.assertIn("ref: main\n          path: trusted", job)
        self.assertIn("path: head", job)
        self.assertEqual(job.count("persist-credentials: false"), job.count("actions/checkout@"))
        self.assertIn("cargo test -p physics --features cross-platform-determinism --test determinism --locked -- golden:: --nocapture", job)
        self.assertIn("python trusted/tools/golden-check/golden_check.py verify --golden trusted/gates/golden/physics-replay.toml", job)
        self.assertNotIn("head/gates", job)
        self.assertNotIn("head/tools", job)
        self.assertNotIn("aarch64", job)

    def test_the_d2_job_can_skip_only_through_the_presence_check_and_only_for_the_cargo_steps(self):
        job = self.d2_job()
        self.assertIn("golden_check.py presence --trusted trusted --head head --github-output", job)
        self.assertIn("lint --golden trusted/gates/golden/physics-replay.toml", job)
        # The lint and the presence check never carry a condition; every other check does and nothing else does.
        steps = job.split("\n      - ")
        for step in steps:
            if "name: The golden file is well formed" in step or "name: Find the physics crate" in step:
                self.assertNotIn("if:", step)
        self.assertEqual(job.count("if: steps.physics.outputs.present == 'true'"), 5)
        self.assertEqual(len(re.findall(r"(?m)^\s+if:", job)), 5)

    def test_the_d2_job_cannot_be_allowed_to_fail_except_the_cost_report(self):
        job = self.d2_job()
        self.assertEqual(job.count("continue-on-error"), 1)
        cost = job[job.index("- name: Cost of the feature"):]
        self.assertIn("continue-on-error: true", cost)
        self.assertNotIn("continue-on-error", job[:job.index("- name: Cost of the feature")])
        self.assertIn("set -euo pipefail", job[job.index("Record the hashes"):job.index("Compare with the golden")])

    def test_the_d2_job_checks_the_build_flags_before_it_builds(self):
        job = self.d2_job()
        self.assertLess(job.index("golden_check.py flags --repo head"), job.index("cargo test -p physics"))

    def tsan_job(self):
        job = CI[CI.index("\n  tsan:") + 1:]
        following = re.search(r"\n  \w+:\n", job)
        return job[:following.start()] if following else job

    def test_the_tsan_job_runs_on_ubuntu_only_with_a_dated_nightly_pinned_in_the_job_env(self):
        # Card 4-21: the pin sits in ci.yml, a protected path, so a bump of the toolchain is a governance diff.
        job = self.tsan_job()
        self.assertIn("runs-on: ubuntu-latest", job)
        self.assertNotIn("matrix", job)
        self.assertRegex(job, r"(?m)^      RUSTUP_TOOLCHAIN: nightly-\d{4}-\d{2}-\d{2}$")
        self.assertEqual(re.findall(r"nightly(?!-\d{4}-\d{2}-\d{2})", job), [], "a floating nightly")
        self.assertIn("--component rust-src", job)
        self.assertIn("rustup toolchain install \"$RUSTUP_TOOLCHAIN\"", job)

    def test_the_tsan_job_builds_the_standard_library_with_the_sanitizer_and_stops_on_the_first_race(self):
        job = self.tsan_job()
        self.assertRegex(job, r"(?m)^      RUSTFLAGS: -Zsanitizer=thread$")
        self.assertIn("TSAN_OPTIONS: halt_on_error=1", job)
        self.assertIn("CARGO_TARGET_DIR: target-tsan", job)
        # The profiles say panic = abort; a rebuilt standard library needs unwinding for the test harness.
        self.assertIn("CARGO_PROFILE_DEV_PANIC: unwind", job)
        self.assertIn("CARGO_PROFILE_RELEASE_PANIC: unwind", job)
        for step in ("cargo test -p jobs --features contract-tests", "cargo test -p physics"):
            line = next(l for l in job.splitlines() if step in l)
            self.assertIn("-Zbuild-std", line)
            self.assertIn("--target x86_64-unknown-linux-gnu", line)
            self.assertIn("--locked", line)

    def test_the_tsan_job_covers_the_pool_and_skips_physics_only_through_the_presence_check(self):
        job = self.tsan_job()
        self.assertIn("--lib --tests", job[job.index("cargo test -p jobs"):])
        for target in ("determinism", "joints", "parallel", "parallel_allocation", "stability"):
            self.assertIn(f"--test {target}", job)
        self.assertIn("golden_check.py presence --trusted trusted --head head --github-output", job)
        self.assertIn("ref: main\n          path: trusted", job)
        self.assertEqual(job.count("persist-credentials: false"), job.count("actions/checkout@"))
        physics = job[job.index("- name: Thread-sanitize the physics tests"):]
        self.assertIn("if: steps.physics.outputs.present == 'true'", physics)
        # Two conditions for physics and four for the audio steps of card 5-12 (checked below).
        self.assertEqual(len(re.findall(r"(?m)^\s+if:", job)), 6)
        # An absent crate is stated in the job summary, never green by silence.
        self.assertIn("not run: physics crate absent", job)
        # The run proves that the binaries are instrumented, before the physics step.
        self.assertIn("grep -q __tsan_init", job)
        self.assertLess(job.index("grep -q __tsan_init"), job.index("- name: Thread-sanitize the physics tests"))
        self.assertNotIn("continue-on-error", job)
        self.assertNotIn("head/gates", job)

    def test_the_tsan_job_covers_the_spsc_queue_and_the_audio_engine_tests_only_through_a_presence_check(self):
        # Card 5-12: the queue (base, tests/spsc.rs) and the audio tests that start threads.
        job = self.tsan_job()
        queue = next(l for l in job.splitlines() if "cargo test -p base" in l)
        engine = next(l for l in job.splitlines() if "cargo test -p audio" in l)
        self.assertIn("--test spsc", queue)
        self.assertIn("--test engine --test engine_threads", engine)
        for line in (queue, engine):
            self.assertIn("-Zbuild-std", line)
            self.assertIn("--target x86_64-unknown-linux-gnu", line)
            self.assertIn("--locked", line)
        audio = job[job.index("- name: Find the audio crate"):job.index("- name: Thread-sanitize the physics tests")]
        # The presence rule is the one of physics: absent from both trees is the only skip, main-only is red.
        self.assertIn("head/engine/audio/Cargo.toml", audio)
        self.assertIn("trusted/engine/audio/Cargo.toml", audio)
        self.assertIn("elif [ -f trusted/engine/audio/Cargo.toml ]", audio)
        self.assertEqual(audio.count("exit 1"), 2, "one red for a missing queue test, one for a crate only on main")
        self.assertEqual(audio.count("set -euo pipefail"), 2, "the presence step and the proof step both stop on error")
        self.assertIn('test -n "$binary"', audio)
        self.assertIn("-perm -u+x", audio)
        self.assertEqual(audio.count("if: steps.audio.outputs.present == 'true'"), 3)
        self.assertEqual(audio.count("if: steps.audio.outputs.present != 'true'"), 1)
        # Not green by silence: the summary says so, and each binary is proved instrumented.
        self.assertIn("not run: audio crate absent", audio)
        for name in ("spsc", "engine", "engine_threads"):
            self.assertIn(name, audio[audio.index("for name in"):audio.index("do\n")])
        self.assertIn("grep -q __tsan_init", audio)
        self.assertLess(audio.index("cargo test -p base"), audio.index("grep -q __tsan_init"))
        self.assertLess(audio.index("cargo test -p audio"), audio.index("grep -q __tsan_init"))

    def icount_job(self):
        job = CI[CI.index("\n  physics_icount:") + 1:]
        following = re.search(r"\n  \w+:\n", job)
        return job[:following.start()] if following else job

    def test_the_icount_job_takes_every_decision_from_a_trusted_main_checkout(self):
        # Card 4-23: the comparator, the bands and the registered workload come from main; the pushed commit is data.
        job = self.icount_job()
        self.assertIn("runs-on: ubuntu-latest", job)
        self.assertNotIn("matrix", job)
        self.assertIn("|| 'main' }}\n          path: trusted", job)
        self.assertRegex(job, r"(?m)^          path: base$")
        self.assertRegex(job, r"(?m)^          path: head$")
        self.assertEqual(job.count("persist-credentials: false"), job.count("actions/checkout@"))
        self.assertEqual(job.count("actions/checkout@"), 3)
        for tree in ("head", "base"):
            self.assertNotIn(f"{tree}/gates", job)
            self.assertNotIn(f"{tree}/tools", job)
        for command in ("presence", "measure", "compare"):
            line = next(l for l in job.splitlines() if f"icount_gate.py {command}" in l)
            self.assertIn("python trusted/tools/icount-gate/icount_gate.py", line)
            self.assertIn("--gates trusted/gates/gates.toml --benchmarks trusted/gates/benchmarks.toml", line)
        self.assertNotIn("continue-on-error", job)

    def test_the_icount_trusted_tree_is_main_before_a_push_never_the_pushed_commit_and_is_locked(self):
        # On a push to main the ref `main` IS the pushed commit; the gate files must come from the tree before it.
        job = self.icount_job()
        self.assertEqual(job.count("github.event.before"), 2)
        lock = job.index("sudo chown -R root:root trusted")
        self.assertIn("sudo chmod -R go-w trusted", job)
        self.assertLess(lock, job.index("cargo build --release"))
        self.assertLess(lock, job.index("id: gate"))
        self.assertLess(job.index("cargo build --release"), job.index("icount_gate.py measure"))

    def test_the_icount_base_is_main_before_the_push_or_the_main_tip(self):
        job = self.icount_job()
        self.assertIn("ref: ${{ github.event_name == 'push' && github.event.before || 'main' }}\n          path: base", job)

    def test_the_icount_job_builds_both_trees_in_one_job_and_measures_both_with_the_trusted_tool(self):
        job = self.icount_job()
        self.assertEqual(job.count('cargo build --release -p "$PACKAGE" --locked'), 2)
        self.assertIn("working-directory: head", job)
        self.assertIn("working-directory: base", job)
        self.assertIn("sudo apt-get install -y -qq valgrind", job)
        self.assertIn('--bench "head/target/release/$PACKAGE"', job)
        self.assertIn('--bench "base/target/release/$PACKAGE"', job)
        # The package name is data of benchmarks.toml and reaches the shell through env, not through an expression.
        for line in (l for l in job.splitlines() if "cargo build" in l or "--bench" in l):
            self.assertNotIn("${{", line)

    def test_the_icount_job_skips_only_through_the_presence_check_and_states_it(self):
        job = self.icount_job()
        presence = next(l for l in job.splitlines() if "icount_gate.py presence" in l)
        self.assertIn('--summary "$GITHUB_STEP_SUMMARY"', presence)
        # Only the presence step can be switched off, and only by the bootstrap check below; the others follow it.
        presence_step = next(s for s in job.split("\n      - ") if "name: Find the benchmark tool" in s)
        self.assertIn("if: steps.gate.outputs.ready == 'true'", presence_step)
        self.assertEqual(job.count("if: steps.bench.outputs.head == 'true'"), 4)
        self.assertEqual(job.count("if: steps.bench.outputs.base == 'true'"), 2)
        self.assertEqual(len(re.findall(r"(?m)^\s+if:", job)), 7)
        # The compare step runs whenever the head has the tool, with or without a base result.
        self.assertIn("HAS_BASE", job)

    def test_the_icount_job_states_the_bootstrap_case_when_main_has_no_gate_tool_yet(self):
        # Before the card that adds the tool is merged, main cannot supply it: stated in the summary, not red and not silent.
        job = self.icount_job()
        step = next(s for s in job.split("\n      - ") if "id: gate" in s)
        self.assertIn("trusted/tools/icount-gate/icount_gate.py", step)
        self.assertIn("trusted/gates/benchmarks.toml", step)
        self.assertIn("not run: icount-gate is not on main yet", step)
        self.assertIn("ready=false", step)
        self.assertNotIn("if:", step)
        self.assertLess(job.index("id: gate"), job.index("id: bench"))

    def audio_icount_job(self):
        job = CI[CI.index("\n  audio_icount:") + 1:]
        following = re.search(r"\n  \w+:\n", job)
        return job[:following.start()] if following else job

    def test_the_audio_icount_job_is_the_physics_job_with_the_audio_profile(self):
        # Card 5-15: same trusted-main, locked-checkout, base-against-head structure; only the profile and the words differ.
        physics, audio = self.icount_job(), self.audio_icount_job()
        for command in ("presence", "measure", "compare"):
            physics = physics.replace(f"icount_gate.py {command} ", f"icount_gate.py {command} --benchmark audio ")
        physics = physics.replace("physics_icount:", "audio_icount:").replace("Physics instruction count", "Audio instruction count")
        physics = physics.replace("[ -f trusted/gates/benchmarks.toml ]; then",
                                  "[ -f trusted/gates/benchmarks.toml ] && grep -q '^\\[audio_icount\\]' trusted/gates/benchmarks.toml; then")
        physics = physics.replace("(tools/icount-gate and gates/benchmarks.toml are absent",
                                  "(the audio profile of tools/icount-gate or [audio_icount] in gates/benchmarks.toml is absent")
        physics = physics.replace("until the card that adds the gate tool is merged", "until the card that adds the audio profile is merged")
        self.assertEqual(audio.strip(), physics.strip())
        self.assertEqual(len(re.findall(r"(?m)^\s+if:", audio)), 7)
        self.assertEqual(audio.count("--benchmark audio"), 4)
        self.assertNotIn("continue-on-error", audio)

    def test_the_audio_icount_job_waits_for_the_audio_table_on_main(self):
        step = next(s for s in self.audio_icount_job().split("\n      - ") if "id: gate" in s)
        self.assertIn("audio_icount", step)
        self.assertIn("ready=false", step)
        self.assertNotIn("if:", step)

    def test_the_icount_tool_states_its_audio_skip_line(self):
        gate = (Path(__file__).resolve().parents[2] / "tools" / "icount-gate" / "icount_gate.py").read_text(encoding="utf-8")
        self.assertIn("not run: audio benchmark tool absent", gate)

    def test_the_icount_tool_states_its_skip_line(self):
        gate = (Path(__file__).resolve().parents[2] / "tools" / "icount-gate" / "icount_gate.py").read_text(encoding="utf-8")
        self.assertIn("not run: physics benchmark tool absent", gate)

    def test_ci_runs_the_tests_of_every_tool_folder(self):
        for folder, module in (("tools/templates", "test_templates"), ("tools/token-probe", "test_probe"),
                               ("tools/local-gates", "test_local_gates"),
                               ("tools/gate-check", "test_governance_guard"),
                               ("tools/gate-check", "test_pr_gates"),
                               ("tools/gate-check", "test_protected_paths"),
                               ("tools/classify-change", "test_classify_change"),
                               ("tools/arch-check", "test_arch_check"),
                               ("tools/wiki-lint", "test_wiki_lint"),
                               ("tools/golden-check", "test_golden_check"),
                               ("tools/icount-gate", "test_icount_gate"),
                               ("tools/metrics", "test_approval_report"),
                               ("acceptance", "test_validate")):
            with self.subTest(folder=folder):
                self.assertIn(f"working-directory: {folder}", CI)
                self.assertIn(module, CI)

    def test_the_gate_steps_run_from_a_trusted_main_checkout_with_the_pr_tree_as_data(self):
        job = CI[CI.index("\n  gates:"):CI.index("\n  metrics:")]
        self.assertIn("ref: main\n          path: trusted", job)
        self.assertIn("path: head", job)
        self.assertEqual(job.count("persist-credentials: false"), job.count("actions/checkout@"))
        self.assertIn("python trusted/tools/arch-check/arch_check.py --config trusted/gates/architecture.toml --repo head", job)
        self.assertIn("python trusted/tools/wiki-lint/wiki_lint.py --config trusted/gates/wiki-lint.toml "
                      "--gates trusted/gates/gates.toml --repo head", job)
        self.assertIn("python trusted/tools/gate-check/check_digest.py --digest head/wiki/rules-digest.md "
                      "--config trusted/gates/gates.toml --claude-md head/CLAUDE.md", job)

    def test_no_gate_tool_runs_from_the_pr_tree_in_the_build_job(self):
        build = CI[CI.index("\n  g1:"):CI.index("\n  gates:")]
        for tool in ("arch_check.py", "wiki_lint.py", "check_digest.py"):
            with self.subTest(tool=tool):
                self.assertNotIn(tool, build)

    def test_the_gate_steps_cannot_be_skipped_or_allowed_to_fail(self):
        job = CI[CI.index("\n  gates:"):CI.index("\n  metrics:")]
        gate_steps = job[job.index("- name: Check the architecture rules"):job.index("# Report only")]
        self.assertNotIn("continue-on-error", gate_steps)
        self.assertNotIn("if:", gate_steps)
        self.assertNotIn("working-directory: head", gate_steps)

    def test_the_gate_tools_are_named_only_in_the_gates_job(self):
        outside = CI[:CI.index("\n  gates:")] + CI[CI.index("\n  metrics:"):]
        for tool in ("arch_check.py", "wiki_lint.py", "check_digest.py", "approval_report.py"):
            with self.subTest(tool=tool):
                self.assertNotIn(tool, outside)

    def test_the_report_step_needs_no_credentials(self):
        job = CI[CI.index("\n  gates:"):CI.index("\n  metrics:")]
        step = job[job.index("- name: Approval report"):]
        self.assertNotIn("git -C head fetch", step)
        self.assertIn("|| diff_hash=0000000000000000", step)

    def test_the_approval_report_is_built_by_main_tools_into_the_job_summary_on_branches_only(self):
        job = CI[CI.index("\n  gates:"):CI.index("\n  metrics:")]
        step = job[job.index("- name: Approval report"):]
        self.assertIn("if: ${{ !cancelled() && github.ref_type == 'branch' && github.ref != 'refs/heads/main' }}", step)
        self.assertIn("python trusted/tools/metrics/approval_report.py", step)
        self.assertIn("--tools-dir trusted/tools --gates-dir trusted/gates --repo head", step)
        self.assertIn("python trusted/tools/diff-hash/diff_hash.py --repo head compute", step)
        self.assertIn('>> "$GITHUB_STEP_SUMMARY"', step)

    def test_ci_runs_the_release_tool_tests(self):
        self.assertIn("working-directory: tools/release", CI)
        for module in ("test_next_version", "test_release_plan", "test_check_pr_title",
                       "test_make_assets", "test_workflows", "test_check_squash_subject"):
            with self.subTest(module=module):
                self.assertIn(module, CI)


HOSTED_LABELS = ("ubuntu-latest", "windows-latest", "ubuntu-24.04-arm", "${{ matrix.os }}")
UNTRUSTED_FIELDS = re.compile(r"\$\{\{\s*github\.(event\.(pull_request|issue|comment|review|head_commit|commits)\b|head_ref)")


def public_repo_violations(text):
    """Why a workflow is unsafe on a public repository (card 4-26); an empty list means it is fine.

    Known limits: a runner label inside a matrix is not read (only the `${{ matrix.os }}` text is accepted; the
    matrices in ci.yml list hosted labels), and the text `secrets.` is refused even in a comment.
    """
    problems = []
    if "secrets." in text:
        problems.append("uses a secret")
    if re.search(r"(?m)^\s*runs-on:.*self-hosted", text):
        problems.append("runs on a self-hosted runner")
    for label in re.findall(r"(?m)^\s*runs-on:\s*(\S.*?)\s*$", text):
        if label not in HOSTED_LABELS and "self-hosted" not in label:
            problems.append("unknown runner " + label)
    if not re.search(r"(?m)^permissions:\n  (contents|pull-requests): (read|write)\n", text):
        problems.append("no top-level permissions block")
    if re.search(r"(?m)^\s*(permissions:\s*)?write-all\s*$", text):
        problems.append("write-all permissions")
    if len(PINNED.findall(text)) != len(USES.findall(text)):
        problems.append("an action is not pinned by commit")
    if any(UNTRUSTED_FIELDS.search(block) for block in run_blocks(text)):
        problems.append("an untrusted event field is expanded inside a shell script")
    return problems


class PublicRepositorySafety(unittest.TestCase):
    """Invariants that must hold on every workflow while the repository is public (card 4-26)."""

    GOOD = (
        "permissions:\n  contents: read\njobs:\n  a:\n    runs-on: ubuntu-latest\n    steps:\n"
        "      - uses: actions/checkout@" + "a" * 40 + "\n      - run: echo hi\n"
    )

    def test_a_clean_workflow_has_no_violation(self):
        self.assertEqual(public_repo_violations(self.GOOD), [])

    def test_a_secret_is_refused(self):
        bad = self.GOOD + "        env:\n          T: ${{ secrets.T }}\n"
        self.assertIn("uses a secret", public_repo_violations(bad))

    def test_a_self_hosted_runner_is_refused(self):
        bad = self.GOOD.replace("ubuntu-latest", "[self-hosted, linux]")
        self.assertIn("runs on a self-hosted runner", public_repo_violations(bad))

    def test_an_unknown_runner_is_refused(self):
        bad = self.GOOD.replace("ubuntu-latest", "my-box")
        self.assertIn("unknown runner my-box", public_repo_violations(bad))

    def test_a_missing_permissions_block_is_refused(self):
        bad = self.GOOD.replace("permissions:\n  contents: read\n", "")
        self.assertIn("no top-level permissions block", public_repo_violations(bad))

    def test_write_all_is_refused(self):
        bad = self.GOOD + "    permissions: write-all\n"
        self.assertIn("write-all permissions", public_repo_violations(bad))

    def test_an_unpinned_action_is_refused(self):
        bad = self.GOOD.replace("a" * 40, "v4")
        self.assertIn("an action is not pinned by commit", public_repo_violations(bad))

    def test_a_pr_title_or_branch_name_inside_a_script_is_refused(self):
        for field in ("github.event.pull_request.title", "github.head_ref"):
            bad = self.GOOD.replace("echo hi", "echo ${{ " + field + " }}")
            self.assertIn("an untrusted event field is expanded inside a shell script", public_repo_violations(bad))

    def test_a_pr_title_passed_through_env_is_fine(self):
        ok = self.GOOD + "      - name: t\n        env:\n          T: ${{ github.event.pull_request.title }}\n        run: echo \"$T\"\n"
        self.assertEqual(public_repo_violations(ok), [])

    def test_every_workflow_of_the_repository_is_safe_on_a_public_repository(self):
        for path in sorted(WORKFLOWS.glob("*.yml")):
            with self.subTest(workflow=path.name):
                self.assertEqual(public_repo_violations(path.read_text(encoding="utf-8")), [])

    def test_only_the_trusted_gates_use_pull_request_target(self):
        for path in sorted(WORKFLOWS.glob("*.yml")):
            if path.name != "trusted-pr-gates.yml":
                with self.subTest(workflow=path.name):
                    self.assertNotIn("pull_request_target", path.read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
