"""Release version computation from Conventional Commit messages.

Every merge produces a version, so a commit that carries no feature or break
(including docs-only and malformed messages) still bumps the patch number.

Feed it the messages of the commits since the last release, separated by NUL:

    git log -z --format=%B <last-tag>..HEAD | python next_version.py --current X.Y.Z

A BREAKING CHANGE footer counts when it starts a line of the body; a footer
quoted in prose on its own line is not told apart from a real one.

A `Release-Batch: <label>` footer (the merge of `develop` into `main`, cards 1-38 and 2-9) bumps at least the minor number.
"""

import argparse
import re
import sys

EXIT_BAD_VERSION = 2
ORDER = ("patch", "minor", "major")
HEADER = re.compile(r"(?P<type>[A-Za-z]+)(\([^)\n]+\))?(?P<bang>!)?:[ \t]+\S")
BREAKING_FOOTER = re.compile(r"^BREAKING[ -]CHANGE: ", re.MULTILINE)
# The merge commit of `develop` carries `Release-Batch: <label>`: that merge is the one minor bump of the batch.
BATCH_FOOTER = re.compile(r"^Release-Batch: \S", re.MULTILINE)
VERSION = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)")


def level(message):
    """Return "major", "minor" or "patch" for one commit message."""
    message = message.lstrip()
    header = HEADER.match(message)
    if BREAKING_FOOTER.search(message) or (header and header["bang"]):
        return "major"
    if header and header["type"].lower() == "feat":
        return "minor"
    return "patch"


def highest_level(messages):
    """Return the highest level over all messages; no messages is a patch."""
    return max((level(message) for message in messages), key=ORDER.index, default="patch")


def next_version(current, messages):
    """Return the version that follows `current` (X.Y.Z) after `messages`."""
    parsed = VERSION.fullmatch(current)
    if not parsed:
        raise ValueError(f"not a plain X.Y.Z version: {current!r}")
    major, minor, patch = (int(part) for part in parsed.groups())
    bump = highest_level(messages)
    batch = any(BATCH_FOOTER.search(message) for message in messages)
    if major == 0:
        # Pre-1.0: a break or a batch merge moves the minor number, everything else the patch number.
        bump = "minor" if bump == "major" or batch else "patch"
    elif batch and bump == "patch":
        bump = "minor"  # after 1.0 a feature is already minor; a batch merge is at least minor
    if bump == "major":
        return f"{major + 1}.0.0"
    if bump == "minor":
        return f"{major}.{minor + 1}.0"
    return f"{major}.{minor}.{patch + 1}"


def run(argv, stdin_text):
    """Return (exit code, stdout text, stderr text)."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--current", required=True, help="latest released version, X.Y.Z")
    args = parser.parse_args(argv)
    try:
        version = next_version(args.current, stdin_text.split("\0"))
    except ValueError as error:
        return EXIT_BAD_VERSION, "", f"{error}\n"
    return 0, version + "\n", ""


def main():
    code, out, err = run(sys.argv[1:], sys.stdin.read())
    sys.stdout.write(out)
    sys.stderr.write(err)
    return code


if __name__ == "__main__":
    sys.exit(main())
