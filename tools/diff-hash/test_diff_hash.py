"""Tests for the diff-hash tool and the approval check (card 0b-5).

The hash is the content hash of the change against main (wiki/workflow/
delivery-pipeline.md section 2). Git behaviour is tested against real throwaway
repositories; the approval parsing is tested on description text.
"""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

import diff_hash

GIT_ENV = {
    **os.environ,
    "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.com",
}

LINES = [f"line {i}\n" for i in range(1, 41)]


class Repo:
    """A throwaway git repository with a `main` branch."""

    def __init__(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.path = Path(self._tmp.name)
        self.git("init", "-q", "-b", "main")
        self.git("config", "core.autocrlf", "false")
        self.git("config", "commit.gpgsign", "false")

    def close(self):
        self._tmp.cleanup()

    def git(self, *args):
        done = subprocess.run(["git", *args], cwd=self.path, env=GIT_ENV, capture_output=True, check=False)
        if done.returncode != 0:
            raise AssertionError(f"git {args} failed: {done.stderr.decode(errors='replace')}")
        return done.stdout.decode(errors="replace").strip()

    def write(self, name, content):
        target = self.path / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(content if isinstance(content, bytes) else content.encode())

    def commit(self, message):
        self.git("add", "-A")
        self.git("commit", "-q", "-m", message)

    def hash(self, base="main", head="HEAD"):
        return diff_hash.compute(base, head, repo=self.path)


def repo_with_base():
    repo = Repo()
    repo.write("a.txt", "".join(LINES))
    repo.commit("base")
    return repo


def edit_line(repo, number, text, name="a.txt"):
    lines = LINES.copy()
    lines[number - 1] = text + "\n"
    repo.write(name, "".join(lines))


class Normalize(unittest.TestCase):
    DIFF = (
        "diff --git a/f b/f\n"
        "index 1111111..2222222 100644\n"
        "--- a/f\n"
        "+++ b/f\n"
        "@@ -10,3 +10,3 @@ fn context()\n"
        " keep\n"
        "-old\n"
        "+new\n"
    )

    def test_index_lines_are_dropped(self):
        self.assertNotIn("index ", diff_hash.normalize(self.DIFF))

    def test_hunk_line_numbers_and_context_label_are_dropped(self):
        text = diff_hash.normalize(self.DIFF)
        self.assertNotIn("-10,3", text)
        self.assertNotIn("fn context()", text)
        self.assertIn("@@\n", text)

    def test_file_headers_and_changed_lines_are_kept(self):
        text = diff_hash.normalize(self.DIFF)
        for kept in ("diff --git a/f b/f", "--- a/f", "+++ b/f", " keep", "-old", "+new"):
            with self.subTest(kept=kept):
                self.assertIn(kept, text)

    def test_unicode_line_separators_in_content_do_not_merge_or_split_lines(self):
        base = "+x\n+y\n"
        for sep in ("\x0c", "\x0b", "\x1c", "\x85", "\r"):
            with self.subTest(sep=repr(sep)):
                self.assertNotEqual(diff_hash.normalize(base), diff_hash.normalize("+x" + sep + "+y\n"))

    def test_carriage_return_at_line_end_is_content(self):
        self.assertNotEqual(diff_hash.normalize("+x\n"), diff_hash.normalize("+x\r\n"))

    def test_content_line_that_looks_like_a_hunk_header_is_kept(self):
        diff = self.DIFF + "+@@ -1,1 +1,1 @@ not a header\n"
        self.assertIn("+@@ -1,1 +1,1 @@ not a header", diff_hash.normalize(diff))


class ComputeHash(unittest.TestCase):
    def setUp(self):
        self.repo = repo_with_base()
        self.addCleanup(self.repo.close)
        self.repo.git("checkout", "-q", "-b", "work")

    def test_is_sixteen_lowercase_hex_characters(self):
        edit_line(self.repo, 30, "changed")
        self.repo.commit("work")
        self.assertRegex(self.repo.hash(), r"^[0-9a-f]{16}$")

    def test_does_not_depend_on_commit_ids_or_messages(self):
        edit_line(self.repo, 30, "changed")
        self.repo.commit("first message")
        first = self.repo.hash()
        self.repo.git("checkout", "-q", "main")
        self.repo.git("checkout", "-q", "-b", "other")
        edit_line(self.repo, 30, "changed")
        self.repo.commit("a completely different message")
        self.assertEqual(self.repo.hash(), first)

    def test_different_content_gives_a_different_hash(self):
        edit_line(self.repo, 30, "changed")
        self.repo.commit("work")
        first = self.repo.hash()
        edit_line(self.repo, 30, "changed again")
        self.repo.commit("work 2")
        self.assertNotEqual(self.repo.hash(), first)

    def test_same_change_in_a_different_file_gives_a_different_hash(self):
        edit_line(self.repo, 30, "changed")
        self.repo.commit("a")
        in_a = self.repo.hash()
        self.repo.git("reset", "-q", "--hard", "main")
        self.repo.write("b.txt", "".join(LINES))
        self.repo.commit("b base")
        base = self.repo.git("rev-parse", "HEAD")
        edit_line(self.repo, 30, "changed", name="b.txt")
        self.repo.commit("b")
        self.assertNotEqual(self.repo.hash(base=base), in_a)

    def test_mode_change_alone_changes_the_hash(self):
        edit_line(self.repo, 30, "changed")
        self.repo.commit("work")
        before = self.repo.hash()
        self.repo.git("update-index", "--chmod=+x", "a.txt")
        self.repo.git("commit", "-q", "-m", "exec")
        self.assertNotEqual(self.repo.hash(), before)

    def test_binary_content_is_part_of_the_hash(self):
        self.repo.write("blob.bin", b"\x00\x01\x02")
        self.repo.commit("bin 1")
        first = self.repo.hash()
        self.repo.write("blob.bin", b"\x00\x01\x03")
        self.repo.commit("bin 2")
        self.assertNotEqual(self.repo.hash(), first)

    def test_option_looking_revisions_are_refused(self):
        for base, head in (("--output=pwned.txt", "HEAD"), ("main", "--output=pwned.txt")):
            with self.subTest(base=base, head=head):
                with self.assertRaises(diff_hash.DiffHashError):
                    diff_hash.compute(base, head, repo=self.repo.path)
        self.assertEqual([p.name for p in self.repo.path.glob("pwned*")], [])

    def test_repository_diff_settings_do_not_change_the_hash(self):
        lines = LINES.copy()
        lines[5 - 1] = "five\n"
        lines[30 - 1] = "thirty\n"
        self.repo.write("a.txt", "".join(lines))
        self.repo.commit("work")
        before = self.repo.hash()
        for key, value in (("diff.context", "1"), ("diff.algorithm", "patience"),
                           ("diff.interHunkContext", "20"), ("diff.indentHeuristic", "false"),
                           ("diff.noprefix", "true"), ("diff.renames", "copies"),
                           ("diff.mnemonicPrefix", "true")):
            with self.subTest(setting=key):
                self.repo.git("config", key, value)
                self.assertEqual(self.repo.hash(), before)
                self.repo.git("config", "--unset", key)

    def test_no_change_is_a_stable_hash_of_nothing(self):
        self.assertRegex(self.repo.hash(), r"^[0-9a-f]{16}$")

    def test_commits_added_to_main_after_the_branch_point_do_not_change_it(self):
        edit_line(self.repo, 30, "changed")
        self.repo.commit("work")
        before = self.repo.hash()
        self.repo.git("checkout", "-q", "main")
        edit_line(self.repo, 2, "main moved on")
        self.repo.commit("main moves")
        self.repo.git("checkout", "-q", "work")
        self.assertEqual(self.repo.hash(), before)


class Rebase(unittest.TestCase):
    def setUp(self):
        self.repo = repo_with_base()
        self.addCleanup(self.repo.close)
        self.repo.git("checkout", "-q", "-b", "work")
        edit_line(self.repo, 30, "my change")
        self.repo.commit("work")
        self.before = self.repo.hash()
        self.repo.git("checkout", "-q", "main")

    def rebase(self):
        self.repo.git("checkout", "-q", "work")
        self.repo.git("rebase", "-q", "main")

    def test_rebase_over_a_distant_change_keeps_the_hash(self):
        lines = ["inserted at the top\n", "and another\n"] + LINES
        self.repo.write("a.txt", "".join(lines))
        self.repo.commit("main inserts lines above, shifting line numbers")
        self.rebase()
        self.assertEqual(self.repo.hash(), self.before)

    def test_rebase_over_an_unrelated_file_keeps_the_hash(self):
        self.repo.write("other.txt", "new file on main\n")
        self.repo.commit("main adds a file")
        self.rebase()
        self.assertEqual(self.repo.hash(), self.before)

    def test_rebase_over_a_change_next_to_the_hunk_changes_the_hash(self):
        # Conservative on purpose: the context around the hunk is part of what was approved.
        lines = LINES.copy()
        lines[27 - 1] = "main edits inside the hunk context\n"
        self.repo.write("a.txt", "".join(lines))
        self.repo.commit("main edits next to the hunk")
        self.rebase()
        self.assertNotEqual(self.repo.hash(), self.before)


def approval(task="0b-5", digest="0123456789abcdef", basis="Both", words='"I approve"'):
    return (
        "Summary of the change.\n\n"
        "## Owner approval\n"
        f"Task: {task}\n"
        f"Diff hash: {digest}\n"
        f"Basis: {basis}\n"
        f"Approved: {words}\n"
    )


HASH = "0123456789abcdef"


class Approval(unittest.TestCase):
    def problems(self, body, computed=HASH, task=None):
        return diff_hash.check_approval(body, computed, task=task)

    def test_matching_record_passes(self):
        self.assertEqual(self.problems(approval()), [])

    def test_backticked_hash_is_accepted(self):
        self.assertEqual(self.problems(approval(digest=f"`{HASH}`")), [])

    def test_mismatch_is_reported_without_echoing_a_body(self):
        problems = self.problems(approval(digest="ffffffffffffffff"))
        self.assertEqual(len(problems), 1)
        self.assertIn("does not match", problems[0])

    def test_missing_section_is_reported(self):
        self.assertIn("no 'Owner approval' section", self.problems("Diff hash: " + HASH + "\n")[0])

    def test_hash_outside_the_section_does_not_count(self):
        body = "Diff hash: " + HASH + "\n\n## Owner approval\nTask: 0b-5\nBasis: Both\nApproved: ok\n"
        self.assertTrue(self.problems(body))

    def test_missing_field_is_reported(self):
        for field in ("Task", "Diff hash", "Basis", "Approved"):
            with self.subTest(field=field):
                body = "\n".join(l for l in approval().splitlines() if not l.startswith(field + ":")) + "\n"
                self.assertTrue(any(field in p for p in self.problems(body)))

    def test_empty_approval_words_are_rejected(self):
        self.assertTrue(self.problems(approval(words="")))

    def test_unknown_basis_is_rejected(self):
        self.assertTrue(any("Basis" in p for p in self.problems(approval(basis="Vibes"))))

    def test_each_valid_basis_is_accepted(self):
        for basis in ("Product view", "Metrics", "Both"):
            with self.subTest(basis=basis):
                self.assertEqual(self.problems(approval(basis=basis)), [])

    def test_short_or_uppercase_hash_is_rejected(self):
        for bad in (HASH[:12], HASH.upper(), HASH + "0"):
            with self.subTest(bad=bad):
                self.assertTrue(self.problems(approval(digest=bad)))

    def test_two_hash_lines_are_rejected(self):
        body = approval() + f"Diff hash: {HASH}\n"
        self.assertTrue(any("exactly one" in p for p in self.problems(body)))

    def test_task_must_match_when_given(self):
        self.assertEqual(self.problems(approval(task="0b-5"), task="0b-5"), [])
    def test_record_inside_a_code_fence_does_not_count(self):
        for fence in ("```", "~~~~"):
            with self.subTest(fence=fence):
                body = "Example:\n" + fence + "\n" + approval(digest=HASH) + fence + "\n"
                self.assertTrue(self.problems(body))

    def test_record_inside_an_unclosed_fence_does_not_count(self):
        self.assertTrue(self.problems("```\n" + approval()))

    def test_record_inside_an_html_comment_does_not_count(self):
        self.assertTrue(self.problems("<!--\n" + approval() + "-->\n"))

    def test_two_approval_sections_are_rejected(self):
        problems = self.problems(approval() + "\n" + approval(digest="ffffffffffffffff"))
        self.assertTrue(any("more than one" in p for p in problems))

    def test_record_next_to_a_fenced_example_still_counts(self):
        body = "```\nexample\n```\n" + approval()
        self.assertEqual(self.problems(body), [])

    def test_crlf_description_is_accepted(self):
        self.assertEqual(self.problems(approval().replace("\n", "\r\n")), [])


    def test_unbalanced_backticks_are_rejected(self):
        for bad in (f"`{HASH}", f"{HASH}`"):
            with self.subTest(bad=bad):
                self.assertTrue(self.problems(approval(digest=bad)))

    def test_section_ends_at_the_next_heading(self):
        body = approval() + "\n## Notes\nDiff hash: ffffffffffffffff\n"
        self.assertEqual(self.problems(body), [])


class CommandLine(unittest.TestCase):
    def setUp(self):
        self.repo = repo_with_base()
        self.addCleanup(self.repo.close)
        self.repo.git("checkout", "-q", "-b", "work")
        edit_line(self.repo, 30, "changed")
        self.repo.commit("work")
        self.body = self.repo.path.parent / f"{self.repo.path.name}-body.md"
        self.addCleanup(lambda: self.body.unlink(missing_ok=True))
        self.out = []

    def run_main(self, *args):
        return diff_hash.main(["--repo", str(self.repo.path), *args], print_line=self.out.append)

    def test_compute_prints_the_hash(self):
        self.assertEqual(self.run_main("compute", "--base", "main", "--head", "HEAD"), 0)
        self.assertEqual(self.out, [self.repo.hash()])

    def test_check_passes_on_a_match(self):
        self.body.write_text(approval(digest=self.repo.hash()), encoding="utf-8")
        code = self.run_main("check", "--base", "main", "--head", "HEAD", "--body-file", str(self.body))
        self.assertEqual(code, 0)

    def test_check_fails_on_a_seeded_mismatch(self):
        self.body.write_text(approval(digest="ffffffffffffffff"), encoding="utf-8")
        code = self.run_main("check", "--base", "main", "--head", "HEAD", "--body-file", str(self.body))
        self.assertEqual(code, 1)
        self.assertTrue(any("does not match" in line for line in self.out))

    def test_check_fails_when_the_change_moves_after_approval(self):
        self.body.write_text(approval(digest=self.repo.hash()), encoding="utf-8")
        edit_line(self.repo, 31, "sneaked in after approval")
        self.repo.commit("late change")
        code = self.run_main("check", "--base", "main", "--head", "HEAD", "--body-file", str(self.body))
        self.assertEqual(code, 1)

    def test_check_fails_on_an_empty_diff(self):
        empty = diff_hash.compute("work", "work", repo=self.repo.path)
        self.body.write_text(approval(digest=empty), encoding="utf-8")
        code = self.run_main("check", "--base", "work", "--head", "work", "--body-file", str(self.body))
        self.assertEqual(code, 1)
        self.assertTrue(any("empty" in line for line in self.out))

    def test_unknown_revision_fails_cleanly(self):
        self.assertEqual(self.run_main("compute", "--base", "nope", "--head", "HEAD"), 1)


if __name__ == "__main__":
    unittest.main()
