"""A throwaway git repository for the release tools' tests."""

import os
import subprocess
import tempfile
from pathlib import Path

GIT_ENV = {
    **os.environ,
    "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.com",
}


class Repo:
    def __init__(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.path = Path(self._tmp.name)
        self.git("init", "-q", "-b", "main")
        self.git("config", "commit.gpgsign", "false")
        self.git("config", "tag.gpgsign", "false")

    def close(self):
        self._tmp.cleanup()

    def git(self, *args):
        done = subprocess.run(["git", *args], cwd=self.path, env=GIT_ENV, capture_output=True, check=False)
        if done.returncode != 0:
            raise AssertionError(f"git {args} failed: {done.stderr.decode(errors='replace')}")
        return done.stdout.decode(errors="replace").strip()

    def commit(self, message, name=None, content=None):
        """Commit one file change (a new file per commit unless `name` is given)."""
        count = len(self.git("rev-list", "--all", "--count").split()) and int(self.git("rev-list", "--all", "--count"))
        target = self.path / (name or f"f{count}.txt")
        target.write_text(content if content is not None else f"{count}\n", encoding="utf-8")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", message)
        return self.git("rev-parse", "HEAD")
