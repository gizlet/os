"""Resolve the build VM's Debian packages into a pinned lock file.

Usage: python3 -I devops/bazel/debian/update_lock.py devops/bazel/debian/packages.json

Reads the snapshot, suite, architecture and requested packages from the JSON
input and rewrites packages.lock.json beside it. The closure contains every
Priority: required package and the Depends/Pre-Depends of the requested
packages. Recommends are not followed. For alternatives and virtual packages
the first resolvable choice in Debian's order wins unless `prefer` names one.
`"required": false` leaves out the Priority: required packages, for a set of
libraries that is not a bootable system.
"""

import json
import lzma
from pathlib import Path
import re
import sys
import urllib.request

SNAPSHOT = "https://snapshot.debian.org/archive/debian/{snapshot}/"


def fetch(url):
    with urllib.request.urlopen(url, timeout=300) as response:
        return response.read()


def parse_packages(text):
    for block in text.split("\n\n"):
        fields, key = {}, None
        for line in block.splitlines():
            if line[:1] in (" ", "\t") and key:
                fields[key] += "\n" + line
            elif ":" in line:
                key, value = line.split(":", 1)
                fields[key] = value.strip()
        if "Package" in fields:
            yield fields


def relations(value):
    """Returns a list of alternative lists of package names, ignoring versions."""
    groups = []
    for group in value.split(","):
        names = [re.sub(r"[\s(\[<].*", "", alt.strip()).split(":")[0] for alt in group.split("|")]
        names = [name for name in names if name]
        if names:
            groups.append(names)
    return groups


def main():
    spec_path = Path(sys.argv[1])
    spec = json.loads(spec_path.read_text())
    base = SNAPSHOT.format(snapshot=spec["snapshot"])
    index = f"{base}dists/{spec['suite']}/main/binary-{spec['architecture']}/Packages.xz"
    packages = {}
    providers = {}
    for fields in parse_packages(lzma.decompress(fetch(index)).decode()):
        name = fields["Package"]
        packages.setdefault(name, fields)
        for virtual in relations(fields.get("Provides", "")):
            providers.setdefault(virtual[0], []).append(name)
    prefer = spec.get("prefer", {})
    exclude = set(spec.get("exclude", []))

    def choose(alternatives, selected):
        for name in alternatives:
            if name in selected:
                return name
        for name in alternatives:
            if name in prefer:
                return prefer[name]
            if name in packages and name not in exclude:
                return name
            candidates = [p for p in providers.get(name, []) if p not in exclude]
            if candidates:
                return next((p for p in candidates if p in selected), sorted(candidates)[0])
        raise SystemExit(f"Unresolvable dependency: {' | '.join(alternatives)}")

    roots = []
    if spec.get("required", True):
        roots = [n for n, f in packages.items() if f.get("Priority") == "required" and n not in exclude]
    roots += spec["packages"]
    selected, queue = set(), list(roots)
    while queue:
        name = choose([queue.pop()], selected)
        if name in selected:
            continue
        selected.add(name)
        fields = packages[name]
        for key in ("Pre-Depends", "Depends"):
            for alternatives in relations(fields.get(key, "")):
                queue.append(choose(alternatives, selected))

    lock = {
        "snapshot": spec["snapshot"],
        "suite": spec["suite"],
        "architecture": spec["architecture"],
        "packages": [
            {
                "name": name,
                "version": packages[name]["Version"],
                "path": packages[name]["Filename"],
                "sha256": packages[name]["SHA256"],
                "size": int(packages[name]["Size"]),
            }
            for name in sorted(selected)
        ],
    }
    out = spec_path.with_name("packages.lock.json")
    out.write_text(json.dumps(lock, indent=1, sort_keys=True) + "\n")
    total = sum(p["size"] for p in lock["packages"])
    print(f"{len(selected)} packages, {total / 1e6:.0f} MB -> {out}")


if __name__ == "__main__":
    main()
