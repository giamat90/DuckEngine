"""Check that a squash commit subject is its PR title plus the PR number (card 0b-8).

The subject on main decides the release version. GitHub Free cannot force the
squash message, so the release workflow compares it with the PR title.

    check_squash_subject.py number --subject "<subject>"      prints the PR number
    check_squash_subject.py check --subject S --title T       exits 1 on a mismatch
"""

import argparse
import re
import sys

import check_pr_title

NUMBER = re.compile(r".* \(#([1-9][0-9]*)\)")


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


def main(argv, print_line=print):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("mode", choices=("number", "check"))
    parser.add_argument("--subject", required=True)
    parser.add_argument("--title")
    args = parser.parse_args(argv)
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
