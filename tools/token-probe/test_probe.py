"""Tests for the scoped-token probe (logic only; no network)."""

import json
import unittest

import probe

SHA = "a" * 40
MAIN_REF = json.dumps({"object": {"sha": SHA}})

REPO = probe.REPO
REF_PATH = f"repos/{REPO}/git/ref/heads/main"
CREATE_PATH = f"repos/{REPO}/git/refs"
DELETE_PATH = f"repos/{REPO}/git/refs/heads/{probe.BRANCH}"
SECRETS_PATH = f"repos/{REPO}/actions/secrets"
ADMIN_PATH = f"repos/{REPO}/actions/permissions"

# What a correctly scoped token gets, keyed by (method, path).
SCOPED = {
    ("GET", f"repos/{REPO}"): probe.Response(200, "{}"),
    ("GET", REF_PATH): probe.Response(200, MAIN_REF),
    ("GET", f"repos/{REPO}/pulls?per_page=1"): probe.Response(200, "[]"),
    ("GET", f"repos/{REPO}/actions/runs?per_page=1"): probe.Response(200, "{}"),
    ("GET", SECRETS_PATH): probe.Response(403, "{}"),
    ("GET", ADMIN_PATH): probe.Response(403, "{}"),
    ("POST", CREATE_PATH): probe.Response(201, "{}"),
    ("DELETE", DELETE_PATH): probe.Response(204, ""),
}


class FakeGitHub:
    def __init__(self, **overrides):
        self.answers = dict(SCOPED)
        for key, response in overrides.items():
            self.answers[self.key_for(key)] = response
        self.calls = []

    @staticmethod
    def key_for(name):
        method, _, path = name.partition("__")
        return (method, {
            "secrets": SECRETS_PATH,
            "admin": ADMIN_PATH,
            "create": CREATE_PATH,
            "delete": DELETE_PATH,
            "ref": REF_PATH,
        }[path])

    def __call__(self, method, path, fields=None):
        self.calls.append((method, path, fields))
        return self.answers[(method, path)]


def by_name(results):
    return {r.name: r for r in results}


class ParseResponse(unittest.TestCase):
    def test_reads_status_and_body_from_include_output(self):
        text = "HTTP/2.0 403 Forbidden\r\nContent-Type: x\r\n\r\n{\"message\":\"no\"}"
        self.assertEqual(probe.parse_response(text), probe.Response(403, '{"message":"no"}'))

    def test_empty_output_is_status_zero(self):
        self.assertEqual(probe.parse_response(""), probe.Response(0, ""))


class ScopedToken(unittest.TestCase):
    def test_every_probe_passes(self):
        results = probe.run(FakeGitHub())
        self.assertTrue(all(r.ok for r in results), results)

    def test_covers_the_expected_probes(self):
        names = {r.name for r in probe.run(FakeGitHub())}
        self.assertEqual(
            names,
            {
                "read-meta", "read-contents", "read-pulls", "read-actions",
                "secrets-denied", "admin-denied",
                "write-branch", "cleanup-branch",
            },
        )

    def test_branch_is_created_from_main_sha(self):
        github = FakeGitHub()
        probe.run(github)
        created = [c for c in github.calls if c[0] == "POST"]
        self.assertEqual(created[0][2], {"ref": f"refs/heads/{probe.BRANCH}", "sha": SHA})

    def test_never_writes_a_workflow_file(self):
        # Workflows is allowed on purpose and cannot be probed without a write (card 0c-8).
        github = FakeGitHub()
        probe.run(github)
        self.assertNotIn("PUT", [c[0] for c in github.calls])
        self.assertFalse([c for c in github.calls if ".github" in c[1]])

    def test_never_writes_to_main(self):
        github = FakeGitHub()
        probe.run(github)
        for method, path, fields in github.calls:
            if method != "GET":
                self.assertNotIn("heads/main", path)
                self.assertNotEqual((fields or {}).get("branch"), "main")
                self.assertNotEqual((fields or {}).get("ref"), "refs/heads/main")
        self.assertTrue(probe.BRANCH.startswith("agent/"))


class BroadToken(unittest.TestCase):
    def test_readable_secrets_fail(self):
        results = by_name(probe.run(FakeGitHub(GET__secrets=probe.Response(200, "{}"))))
        self.assertFalse(results["secrets-denied"].ok)

    def test_readable_admin_settings_fail(self):
        results = by_name(probe.run(FakeGitHub(GET__admin=probe.Response(200, "{}"))))
        self.assertFalse(results["admin-denied"].ok)


class TokenMissingPermissions(unittest.TestCase):
    def test_unanswered_create_still_attempts_cleanup(self):
        github = FakeGitHub(POST__create=probe.Response(0, ""))
        results = by_name(probe.run(github))
        self.assertFalse(results["write-branch"].ok)
        self.assertIn("DELETE", [c[0] for c in github.calls])

    def test_cannot_create_branch_fails_and_skips_dependents(self):
        github = FakeGitHub(POST__create=probe.Response(403, "{}"))
        results = by_name(probe.run(github))
        self.assertFalse(results["write-branch"].ok)
        self.assertNotIn("DELETE", [c[0] for c in github.calls])

    def test_leftover_probe_branch_is_cleaned_up(self):
        github = FakeGitHub(POST__create=probe.Response(422, "{}"))
        results = by_name(probe.run(github))
        self.assertFalse(results["write-branch"].ok)
        self.assertIn("DELETE", [c[0] for c in github.calls])

    def test_unreadable_main_fails_cleanly(self):
        github = FakeGitHub(GET__ref=probe.Response(403, "{}"))
        results = by_name(probe.run(github))
        self.assertFalse(results["read-contents"].ok)
        self.assertFalse(results["write-branch"].ok)
        self.assertNotIn("POST", [c[0] for c in github.calls])

    def test_malformed_ref_body_fails_cleanly(self):
        github = FakeGitHub(GET__ref=probe.Response(200, "not json"))
        results = by_name(probe.run(github))
        self.assertFalse(results["write-branch"].ok)


class Report(unittest.TestCase):
    def test_lines_show_names_and_statuses_only(self):
        lines = probe.format_results(probe.run(FakeGitHub()))
        self.assertIn("PASS read-meta (HTTP 200)", lines)
        self.assertIn("PASS secrets-denied (HTTP 403)", lines)
        self.assertNotIn(SHA, "\n".join(lines))

    def test_not_run_probe_is_reported_as_such(self):
        results = probe.run(FakeGitHub(GET__ref=probe.Response(403, "{}")))
        self.assertIn("FAIL write-branch (not run)", probe.format_results(results))

    def test_exit_code_follows_the_results(self):
        self.assertEqual(probe.main(FakeGitHub(), print_line=lambda _: None), 0)
        broad = FakeGitHub(GET__secrets=probe.Response(200, "{}"))
        self.assertEqual(probe.main(broad, print_line=lambda _: None), 1)


if __name__ == "__main__":
    unittest.main()
