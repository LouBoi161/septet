#!/usr/bin/env python3
"""Write THIRD-PARTY-LICENSES.md: every crate Septet is built from, its license, and the license
texts (with the copyright lines) shipped in each crate. Run from the repository root after a build
(so cargo has the sources): `python3 scripts/third_party_licenses.py [--target <triple>] > out.md`.
"""

import hashlib
import json
import os
import subprocess
import sys
from collections import defaultdict

LICENSE_PREFIXES = ("license", "licence", "copying", "notice", "unlicense", "copyright")

MIT = """Copyright (c) the authors named for each component

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
associated documentation files (the "Software"), to deal in the Software without restriction,
including without limitation the rights to use, copy, modify, merge, publish, distribute,
sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or
substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT
NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES
OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE."""


def main() -> None:
    sys.stdout.reconfigure(encoding="utf-8")  # Windows defaults to the ANSI code page
    args = ["cargo", "metadata", "--format-version", "1", "--locked"]
    if "--target" in sys.argv:
        args += ["--filter-platform", sys.argv[sys.argv.index("--target") + 1]]
    meta = json.loads(subprocess.run(args, check=True, capture_output=True, text=True, encoding="utf-8").stdout)
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    root = next(p["id"] for p in meta["packages"] if p["name"] == "septet")

    # Everything linked into the program: normal and build dependencies, not dev-dependencies.
    seen, stack = set(), [root]
    while stack:
        pid = stack.pop()
        if pid in seen:
            continue
        seen.add(pid)
        for dep in nodes[pid]["deps"]:
            if any(k["kind"] in (None, "build") for k in dep["dep_kinds"]):
                stack.append(dep["pkg"])
    seen.discard(root)

    texts = {}  # hash -> text
    bare = []  # crates that ship no license file: (name, version, license, authors)
    users = defaultdict(list)  # hash -> ["crate version"]
    rows = []
    for pid in sorted(seen, key=lambda i: (packages[i]["name"], packages[i]["version"])):
        p = packages[pid]
        name, version = p["name"], p["version"]
        license_ = p.get("license") or ("see " + p["license_file"] if p.get("license_file") else "unspecified")
        rows.append((name, version, license_, p.get("repository") or ""))
        files = license_files(p)
        if not files:
            bare.append((name, version, license_, ", ".join(p.get("authors") or []) or "its authors"))
        for path in files:
            try:
                with open(path, encoding="utf-8", errors="replace") as f:
                    text = f.read().strip()
            except OSError:
                continue
            digest = hashlib.sha256(text.encode()).hexdigest()
            texts[digest] = text
            users[digest].append(f"{name} {version}")

    out = sys.stdout
    out.write("# Third-party licenses\n\n")
    out.write("Septet (PolyForm Noncommercial 1.0.0) and the seven apps it bundles (MIT OR Apache-2.0) are built ")
    out.write(f"from the {len(rows)} components below. Each one's license terms follow the table; ")
    out.write("where a component offers a choice of licenses, Septet uses it under the permissive option ")
    out.write("(MIT or Apache-2.0).\n\n")
    out.write("| Component | Version | License | Source |\n|---|---|---|---|\n")
    for name, version, license_, repo in rows:
        out.write(f"| {name} | {version} | {license_} | {repo} |\n")
    out.write("\n# License texts\n\n")
    for digest, text in sorted(texts.items(), key=lambda kv: sorted(users[kv[0]])[0]):
        names = sorted(set(users[digest]))
        shown = ", ".join(names[:12]) + (f" and {len(names) - 12} more" if len(names) > 12 else "")
        out.write(f"## {shown}\n\n```text\n{text}\n```\n\n")
    if bare:
        out.write("# Components without a bundled license file\n\n")
        out.write("These components declare their license in their package metadata only. Their terms are the ")
        out.write("standard texts below, with the copyright held by the authors named here.\n\n")
        out.write("| Component | Version | License | Copyright |\n|---|---|---|---|\n")
        for name, version, license_, authors in bare:
            out.write(f"| {name} | {version} | {license_} | {authors} |\n")
        here = os.path.dirname(os.path.abspath(__file__))
        apache = os.path.join(here, "..", "photocraft", "LICENSE-APACHE")
        out.write("\n## MIT License (standard text)\n\n```text\n" + MIT + "\n```\n\n")
        if os.path.isfile(apache):
            with open(apache, encoding="utf-8") as f:
                out.write("## Apache License 2.0 (standard text)\n\n```text\n" + f.read().strip() + "\n```\n\n")
    fonts = os.environ.get("CRAFT_FONTS_DIR")
    if fonts:
        write_fonts(out, fonts)


def write_fonts(out, root: str) -> None:
    """The craft-fonts faces the apps embed (CRAFT_FONTS_DIR), with each one's license text."""
    with open(os.path.join(root, "fonts", "manifest.txt"), encoding="utf-8") as f:
        lines = [line.strip() for line in f if line.strip() and not line.startswith("#")]
    out.write("# Bundled fonts\n\n")
    out.write("The apps embed these fonts from [craft-fonts](https://github.com/storytold/craft-fonts), unmodified.\n\n")
    out.write("| Font | Style | License | Source |\n|---|---|---|---|\n")
    licenses = {}  # license file -> families
    for line in lines:
        family, style, _, _, license_, license_file, _, source = (field.strip() for field in line.split(" | "))
        out.write(f"| {family} | {style} | {license_} | {source} |\n")
        licenses.setdefault(license_file, [])
        if family not in licenses[license_file]:
            licenses[license_file].append(family)
    out.write("\n")
    for license_file, families in licenses.items():
        with open(os.path.join(root, license_file), encoding="utf-8") as f:
            out.write(f"## {', '.join(families)}\n\n```text\n{f.read().strip()}\n```\n\n")


def license_files(package: dict) -> list:
    """License files in the crate's folder, or, for crates in a workspace, the workspace root's."""
    folder = os.path.dirname(package["manifest_path"])
    found = []
    for candidate in (folder, os.path.dirname(os.path.dirname(folder)), os.path.dirname(os.path.dirname(os.path.dirname(folder)))):
        try:
            entries = sorted(os.listdir(candidate))
        except OSError:
            continue
        found = [os.path.join(candidate, e) for e in entries if e.lower().startswith(LICENSE_PREFIXES) and os.path.isfile(os.path.join(candidate, e))]
        if found or candidate != folder or "/.cargo/registry/" in folder or "/.cargo/git/" in folder:
            break
    if package.get("license_file"):
        path = os.path.join(folder, package["license_file"])
        if os.path.isfile(path) and path not in found:
            found.append(path)
    return found


if __name__ == "__main__":
    main()
