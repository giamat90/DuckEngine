"""Check that a squash commit subject is its PR title plus the PR number (card 0b-8).

The subject on main decides the release version. GitHub Free cannot force the
squash message, so the release workflow compares it with the PR title.

    check_squash_subject.py number --subject "<subject>"      prints the PR number
    check_squash_subject.py check --subject S --title T       exits 1 on a mismatch
    check_squash_subject.py footer --head-ref R --message M   exits 1 on a wrong Release-Batch footer

A release merge (head branch `develop`) must carry exactly one `Release-Batch: <label>` footer, because next_version.py
turns it into the one minor bump of the batch (cards 1-38, 2-9); any other merge must not carry it (card 1-40).
"""

import argparse
import re
import sys

import check_pr_title

NUMBER = re.compile(r".* \(#([1-9][0-9]*)\)")
RELEASE_BRANCH = "develop"
ANY_FOOTER = re.compile(r"^Release-Batch: \S", re.MULTILINE)  # the line next_version.py reads
FOOTER = re.compile(r"^Release-Batch: (\S+)[ \t\r]*$", re.MULTILINE)


def pr_number(subject):
    match = NUMBER.fullmatch(subject)
    return int(match.group(1)) if match and match.group(1).isascii() else None


def problems(subject, title):
    number = pr_number(subject)
    if number is None:
        return [f"subject '{subject}' does not end with ' (#<pr number>)'; was the PR squash-merged?"]
    found = check_pr_title.problems(title)
    expected = f"{title} (#{number})"
    if not found and subject != expected:
        found = [f"subject '{subject}' is not the PR title: expected '{expected}'"]
    return found


def footer_problems(head_ref, message, batch_only=False):
    """Problems with the Release-Batch footer of `message` for a merge of branch `head_ref`."""
    footers = FOOTER.findall(message)
    lines = len(ANY_FOOTER.findall(message))  # a line the strict form misses still bumps the version
    if head_ref != RELEASE_BRANCH:
        if lines and not batch_only:
            return [f"Release-Batch footer on a merge of '{head_ref}', which is not the release branch '{RELEASE_BRANCH}'"]
        return []
    if lines != 1 or len(footers) != 1:
        return [f"merge of '{head_ref}' needs exactly one footer 'Release-Batch: <label>', found {footers or 'none'}"]
    return []


def main(argv, print_line=print):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("mode", choices=("number", "check", "footer"))
    parser.add_argument("--subject")
    parser.add_argument("--title")
    parser.add_argument("--head-ref")
    parser.add_argument("--message")
    parser.add_argument("--batch-only", action="store_true", help="footer: ignore branches other than the release branch")
    args = parser.parse_args(argv)
    if args.mode == "footer":
        if args.head_ref is None or args.message is None:
            parser.error("footer needs --head-ref and --message")
        found = footer_problems(args.head_ref, args.message, args.batch_only)
        for problem in found:
            print_line(f"FAIL {problem}")
        if not found:
            print_line("OK Release-Batch footer is right for this merge")
        return 1 if found else 0
    if args.subject is None:
        parser.error(f"{args.mode} needs --subject")
    if args.mode == "number":
        number = pr_number(args.subject)
        if number is None:
            print_line(f"FAIL {problems(args.subject, '')[0]}")
            return 1
        print_line(str(number))
        return 0
    if args.title is None:
        parser.error("check needs --title")
    found = problems(args.subject, args.title)
    for problem in found:
        print_line(f"FAIL {problem}")
    if not found:
        print_line("OK squash subject is the PR title")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
