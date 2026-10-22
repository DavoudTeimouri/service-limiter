#!/usr/bin/env python3
"""Extract one version's section from CHANGELOG.md.

Used to build the GitHub Release body from the changelog, so the release
notes and the changelog can never drift apart.

    python scripts/extract_changelog.py 2.1.0-rc.1 > release-notes.md
    gh release create v2.1.0-rc.1 --notes-file release-notes.md --prerelease

Exits non-zero when the version has no section: a release must fail closed
rather than publish an empty body.
"""

import re
import sys


def extract_version(version: str, changelog_path: str = "CHANGELOG.md") -> str:
    with open(changelog_path, "r", encoding="utf-8") as f:
        content = f.read()

    # Escape the version: "2.1.0-rc.1" contains dots and a dash.
    esc = re.escape(version)
    pattern = rf"(##\s*\[{esc}\]\s*-\s*\d{{4}}-\d{{2}}-\d{{2}}\n.*?)(?=\n##\s*\[|\Z)"
    match = re.search(pattern, content, re.DOTALL)
    if not match:
        raise SystemExit(f"Version {version} not found in {changelog_path}")

    return match.group(1).rstrip()


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: extract_changelog.py <version> [changelog_path]", file=sys.stderr)
        sys.exit(1)
    version = sys.argv[1]
    path = sys.argv[2] if len(sys.argv) > 2 else "CHANGELOG.md"
    print(extract_version(version, path))
