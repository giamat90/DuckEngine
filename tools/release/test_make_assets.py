"""Tests for the release asset builder (card 0b-6): dummy artifact, checksums, report, baselines."""

import hashlib
import json
import tarfile
import tempfile
import unittest
from pathlib import Path

import make_assets
from repo_fixture import Repo

EXPECTED = {
    "duckengine-v0.2.0-source.tar.gz",
    "dummy-artifact.txt",
    "metric-report.md",
    "baselines.json",
    "SHA256SUMS",
}


class Build(unittest.TestCase):
    def setUp(self):
        self.repo = Repo()
        self.addCleanup(self.repo.close)
        self.repo.commit("feat: tracked", name="tracked.txt", content="tracked\n")
        (self.repo.path / "untracked.txt").write_text("not committed\n", encoding="utf-8")
        self.commit = self.repo.git("rev-parse", "HEAD")
        self._out = tempfile.TemporaryDirectory()
        self.addCleanup(self._out.cleanup)
        self.out = Path(self._out.name) / "dist"
        self.files = make_assets.build(self.repo.path, "0.2.0", self.out)

    def test_writes_exactly_the_expected_files(self):
        self.assertEqual({p.name for p in self.out.iterdir()}, EXPECTED)
        self.assertEqual({p.name for p in self.files}, EXPECTED)

    def test_checksums_cover_every_other_file_and_are_correct(self):
        lines = (self.out / "SHA256SUMS").read_text(encoding="utf-8").splitlines()
        listed = {}
        for line in lines:
            digest, _, name = line.partition("  ")
            listed[name] = digest
        self.assertEqual(set(listed), EXPECTED - {"SHA256SUMS"})
        for name, digest in listed.items():
            with self.subTest(name=name):
                self.assertEqual(hashlib.sha256((self.out / name).read_bytes()).hexdigest(), digest)

    def test_source_archive_holds_tracked_files_under_a_versioned_folder(self):
        with tarfile.open(self.out / "duckengine-v0.2.0-source.tar.gz") as archive:
            names = archive.getnames()
        self.assertIn("duckengine-v0.2.0/tracked.txt", names)
        self.assertNotIn("duckengine-v0.2.0/untracked.txt", names)

    def test_baselines_record_version_and_commit_outside_git(self):
        data = json.loads((self.out / "baselines.json").read_text(encoding="utf-8"))
        self.assertEqual((data["version"], data["commit"]), ("0.2.0", self.commit))

    def test_dummy_artifact_and_report_say_they_are_placeholders(self):
        for name in ("dummy-artifact.txt", "metric-report.md"):
            with self.subTest(name=name):
                text = (self.out / name).read_text(encoding="utf-8")
                self.assertIn("placeholder", text.lower())
                self.assertIn("0.2.0", text)

    def test_extra_files_are_included_and_checksummed(self):
        extra = self.out.parent / "CHANGELOG.md"
        extra.write_text("- change\n", encoding="utf-8")
        out = self.out.parent / "with-extra"
        make_assets.build(self.repo.path, "0.2.0", out, extra=[extra])
        self.assertTrue((out / "CHANGELOG.md").exists())
        self.assertIn("  CHANGELOG.md\n", (out / "SHA256SUMS").read_text(encoding="utf-8"))

    def test_invalid_version_is_refused_before_anything_is_written(self):
        out = self.out.parent / "other"
        for version in ("v0.2.0", "0.2", "0.2.0-rc1", "", "../0.2.0"):
            with self.subTest(version=version):
                with self.assertRaises(ValueError):
                    make_assets.build(self.repo.path, version, out)
        self.assertFalse(out.exists())


class CommandLine(unittest.TestCase):
    def test_prints_the_file_names(self):
        repo = Repo()
        self.addCleanup(repo.close)
        repo.commit("fix: x")
        with tempfile.TemporaryDirectory() as tmp:
            out = []
            code = make_assets.main(
                ["--repo", str(repo.path), "--version", "0.1.0", "--out", str(Path(tmp) / "d")], print_line=out.append
            )
        self.assertEqual(code, 0)
        self.assertIn("SHA256SUMS", out)

    def test_git_failure_reports_the_git_message(self):
        out = []
        with tempfile.TemporaryDirectory() as tmp:  # not a repository
            code = make_assets.main(["--repo", tmp, "--version", "0.1.0", "--out", str(Path(tmp) / "d")], print_line=out.append)
        self.assertEqual(code, 1)
        self.assertIn("not a git repository", out[0])

    def test_bad_version_exits_one(self):
        out = []
        self.assertEqual(make_assets.main(["--version", "x", "--out", "unused"], print_line=out.append), 1)


if __name__ == "__main__":
    unittest.main()
