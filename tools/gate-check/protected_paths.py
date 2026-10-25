"""The one path matcher of the governance guard and the change classifier (card 0c-9).

gates/protected-paths.txt has one entry per line, `#` starts a comment line:

    dir/        everything below that directory, from the repository root
    file        exactly that path, from the repository root
    **/name     a file called `name` at any depth, the root included
    **/name/    a directory called `name` anywhere in the path

Matching ignores case (Windows does) and treats every path that can alias another file on
Windows (short names, trailing dot or space, stream suffix, backslash, any non-ASCII name: NTFS folds
case its own way) as protected.
"""

import re

ALIAS_PART = re.compile(r"[. ]$|~\d")
ANY_DEPTH = "**/"


def load_protected(text):
    entries = [line.strip() for line in text.splitlines() if line.strip() and not line.lstrip().startswith("#")]
    if not entries:
        raise ValueError("the protected-paths list is empty")
    for entry in entries:
        if entry.startswith(("./", "/")) or "\\" in entry or " #" in entry:
            raise ValueError(f"malformed protected-paths entry '{entry}' (it would never match)")
        name = entry[len(ANY_DEPTH):].rstrip("/") if entry.startswith(ANY_DEPTH) else entry
        if entry.startswith(ANY_DEPTH) and (not name or "/" in name):
            raise ValueError(f"malformed protected-paths entry '{entry}' (use **/name or **/name/)")
        if "*" in name:
            raise ValueError(f"malformed protected-paths entry '{entry}' (only a leading **/ is supported)")
        body = name[:-1] if name.endswith("/") else name
        if ":" in body or any(part in ("", ".", "..") or part[-1] in ". " for part in body.split("/")):
            raise ValueError(f"malformed protected-paths entry '{entry}' (it would never match)")
    return entries


def is_alias(path):
    """True when the path can name another file on Windows, so a plain comparison would miss it."""
    return not path.isascii() or "\\" in path or ":" in path or any(ALIAS_PART.search(part) for part in path.casefold().split("/"))


def _entry_matches(parts, folded, entry):
    if entry.startswith(ANY_DEPTH):
        name = entry[len(ANY_DEPTH):]
        if name.endswith("/"):
            return name[:-1] in parts[:-1]
        return parts[-1] == name
    return folded.startswith(entry) if entry.endswith("/") else folded == entry


def is_protected(path, protected):
    if is_alias(path):
        return True
    folded = path.casefold()
    parts = folded.split("/")
    return any(_entry_matches(parts, folded, entry.casefold()) for entry in protected)
