#!/usr/bin/env python3
"""Write assets/licences.json for the About window: every crate built into the
app (all target platforms, dev-dependencies excluded) with its version, licence,
homepage and licence text.

A crate's own licence files are used where it ships them; otherwise the
standard SPDX texts for its licence expression. The standard texts are
downloaded from the SPDX licence list. Run after changing dependencies; a test
checks the list matches Cargo.lock."""
import json
import os
import re
import subprocess
import urllib.request

root_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SPDX_URL = "https://raw.githubusercontent.com/spdx/license-list-data/main/text/{}.txt"

meta = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--format-version", "1", "--locked"], cwd=root_dir))
pkgs = {p["id"]: p for p in meta["packages"]}
nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}

seen, stack = set(), [meta["resolve"]["root"]]
while stack:
    cur = stack.pop()
    for dep in nodes[cur]["deps"]:
        if {k["kind"] for k in dep["dep_kinds"]} <= {"dev"}:
            continue
        if dep["pkg"] not in seen:
            seen.add(dep["pkg"])
            stack.append(dep["pkg"])


def spdx_ids(expr):
    """The licence and exception identifiers in an SPDX expression."""
    ids = []
    for tok in re.split(r"[\s()/]+", expr):
        if tok and tok not in ("OR", "AND", "WITH") and tok not in ids:
            ids.append(tok)
    return ids


LICENCE_FILE = re.compile(r"(?i)^(licen[cs]e|copying|notice|unlicense)")
texts, text_index = [], {}


def add_text(t):
    t = t.strip().replace("\r\n", "\n")
    if t not in text_index:
        text_index[t] = len(texts)
        texts.append(t)
    return text_index[t]


crates, used_ids = [], set()
for pid in seen:
    p = pkgs[pid]
    lic = p.get("license") or ""
    d = os.path.dirname(p["manifest_path"])
    files = sorted(f for f in os.listdir(d) if LICENCE_FILE.match(f) and os.path.isfile(os.path.join(d, f)))
    if p.get("license_file"):
        files.append(p["license_file"])
    own = []
    for f in dict.fromkeys(files):
        with open(os.path.join(d, f), errors="replace") as fh:
            own.append(add_text(fh.read()))
    ids = spdx_ids(lic)
    used_ids.update(ids)
    if not lic:
        lic = "see licence file" if own else "unknown"
    url = p.get("repository") or p.get("homepage") or f"https://crates.io/crates/{p['name']}"
    crates.append([p["name"], p["version"], lic, url, own])
crates.sort(key=lambda c: (c[0].lower(), c[1]))

spdx = {}
for i in sorted(used_ids):
    with urllib.request.urlopen(SPDX_URL.format(i), timeout=30) as r:
        spdx[i] = r.read().decode("utf-8").strip()

with open(os.path.join(root_dir, "assets", "licences.json"), "w") as f:
    json.dump({"spdx": spdx, "texts": texts, "crates": crates}, f, ensure_ascii=False, separators=(",", ":"))
print(f"{len(crates)} crates, {len(texts)} distinct licence files, {len(spdx)} standard licences")
