#!/usr/bin/env python3
"""Collect license notices from checksum-verified dependency source archives.

--refresh downloads public package archives, never executes or installs them.
--check is offline and rejects stale locks, missing notices, or changed notice files.
Requires Python 3.12+, matching the Hub project requirement.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import io
import json
import re
import tarfile
import tomllib
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path, PurePosixPath
from urllib.error import HTTPError
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / "licenses" / "third-party"
LOCKS = ("worker/Cargo.lock", "uv.lock", "hub/package-lock.json", "pyproject.toml", "hub/package.json")
NOTICE = re.compile(r"^(licen[cs]e|copying|notice|copyright|authors)([._-]|$)", re.I)
LEGACY_NOTICE_REVISIONS = {
    # The crate's VCS revision is no longer reachable upstream; use the
    # published v0.5.0 repository tag containing this package and its notices.
    ("system-configuration-sys", "0.5.0"): "1bc323a12552359f5e26b56d52aa29b3449c09ec",
    # winapi-rs 0.3.9 includes these unchanged 0.4.0 import-library crates.
    ("winapi-i686-pc-windows-gnu", "0.4.0"): "796a8e6c2971dc2ff1bcff166e6671284f9b5b6b",
    ("winapi-x86_64-pc-windows-gnu", "0.4.0"): "796a8e6c2971dc2ff1bcff166e6671284f9b5b6b",
}


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def input_digest(path: str) -> str:
    # Git may check out text lockfiles with CRLF on Windows. This does not
    # change dependency resolution; archive and notice hashes remain byte-exact.
    return digest((ROOT / path).read_bytes().replace(b"\r\n", b"\n"))


def download(url: str) -> bytes:
    if not url.startswith("https://"):
        raise ValueError("Only HTTPS package archives are supported")
    request = Request(url, headers={"User-Agent": "CodeIntelligence-LicenseCollector/1.0"})
    with urlopen(request, timeout=90) as response:
        data = response.read(100 * 1024 * 1024 + 1)
    if len(data) > 100 * 1024 * 1024:
        raise ValueError("Archive exceeds size limit")
    return data


def packages() -> list[dict]:
    rows = []
    cargo = tomllib.loads((ROOT / "worker/Cargo.lock").read_text(encoding="utf-8"))
    for package in cargo["package"]:
        if "source" not in package:
            continue
        if package["source"] != "registry+https://github.com/rust-lang/crates.io-index":
            raise ValueError(f"Unreviewed Cargo source: {package['name']}")
        name, version = package["name"], package["version"]
        rows.append(dict(ecosystem="cargo", name=name, version=version,
                         source_url=f"https://static.crates.io/crates/{name}/{name}-{version}.crate",
                         source_sha256=package["checksum"]))
    python = tomllib.loads((ROOT / "uv.lock").read_text(encoding="utf-8"))
    for package in python["package"]:
        if "registry" not in package.get("source", {}):
            continue
        source = package["sdist"]
        rows.append(dict(ecosystem="pypi", name=package["name"], version=package["version"],
                         source_url=source["url"], source_sha256=source["hash"].removeprefix("sha256:")))
    npm = json.loads((ROOT / "hub/package-lock.json").read_text(encoding="utf-8"))
    for path, package in npm["packages"].items():
        if not path:
            continue
        rows.append(dict(ecosystem="npm", name=path.rsplit("node_modules/", 1)[1],
                         version=package["version"], source_url=package["resolved"],
                         integrity=package["integrity"], license=package.get("license", "")))
    return sorted(rows, key=lambda row: (row["ecosystem"], row["name"], row["version"]))


def collect(row: dict) -> dict:
    data = download(row["source_url"])
    if "source_sha256" in row and digest(data) != row["source_sha256"]:
        raise ValueError(f"Archive checksum mismatch: {row['name']}")
    if "integrity" in row:
        algorithm, expected = row["integrity"].split("-", 1)
        actual = base64.b64encode(hashlib.new(algorithm, data).digest()).decode()
        if actual != expected:
            raise ValueError(f"npm integrity mismatch: {row['name']}")
    row["source_sha256"] = digest(data)
    folder = DEST / row["ecosystem"] / f"{row['name'].replace('/', '__')}-{row['version']}"
    notices = []
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        for member in archive.getmembers():
            parts = PurePosixPath(member.name).parts
            if not member.isfile() or not NOTICE.match(PurePosixPath(member.name).name):
                continue
            if member.size > 2 * 1024 * 1024 or ".." in parts or member.name.startswith("/"):
                raise ValueError(f"Unsafe notice archive path: {member.name}")
            relative = PurePosixPath(*parts[1:])
            content = archive.extractfile(member).read()
            if b"\0" in content:
                continue
            destination = folder.joinpath(*relative.parts)
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(content)
            notices.append(dict(path=destination.relative_to(ROOT).as_posix(), sha256=digest(content)))
    if not notices and row["ecosystem"] == "cargo":
        # Some crates omit root notices from their published archive. Use the
        # exact upstream revision embedded in the checksum-verified crate.
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
            members = {m.name.split("/", 1)[-1]: m for m in archive.getmembers()}
            package = tomllib.loads(archive.extractfile(members["Cargo.toml"]).read().decode())["package"]
            repository = package.get("repository", "").removesuffix(".git")
            if (row["name"], row["version"]) in LEGACY_NOTICE_REVISIONS:
                revision = LEGACY_NOTICE_REVISIONS[(row["name"], row["version"])]
            elif ".cargo_vcs_info.json" in members:
                revision = json.loads(archive.extractfile(members[".cargo_vcs_info.json"]).read())["git"]["sha1"]
            else:
                revision = LEGACY_NOTICE_REVISIONS.get((row["name"], row["version"]))
        if repository.startswith("https://github.com/") and revision:
            for name in ("LICENSE", "LICENSE-MIT", "LICENSE-APACHE"):
                url = repository.replace("https://github.com/", "https://raw.githubusercontent.com/") + f"/{revision}/{name}"
                try:
                    content = download(url)
                except HTTPError as error:
                    if error.code == 404:
                        continue
                    raise
                destination = folder / name
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes(content)
                notices.append(dict(path=destination.relative_to(ROOT).as_posix(), sha256=digest(content), source_url=url))
    if not notices:
        raise ValueError(f"No license notices found for {row['name']} {row['version']}; manual review required")
    row["notices"] = sorted(notices, key=lambda item: item["path"])
    return row


def constraints() -> str:
    versions = {}
    for row in packages():
        if row["ecosystem"] == "pypi":
            if row["name"] in versions and versions[row["name"]] != row["version"]:
                raise ValueError("Multiple Python versions need a marker-aware constraints export")
            versions[row["name"]] = row["version"]
    return "# Generated from uv.lock by scripts/third-party-licenses.py --refresh\n" + "".join(
        f"{name}=={version}\n" for name, version in sorted(versions.items()))


def check() -> None:
    manifest = json.loads((DEST / "manifest.json").read_text(encoding="utf-8"))
    for path in LOCKS:
        if input_digest(path) != manifest["inputs"].get(path):
            raise ValueError(f"Stale license inventory: {path}; run --refresh and review notices")
    expected = {(row["ecosystem"], row["name"], row["version"]) for row in packages()}
    observed = {(row["ecosystem"], row["name"], row["version"]) for row in manifest["packages"]}
    if observed != expected:
        raise ValueError("Dependency inventory does not match lockfiles")
    for row in manifest["packages"]:
        if not row["notices"]:
            raise ValueError(f"No notices: {row['name']}")
        for notice in row["notices"]:
            if digest((ROOT / notice["path"]).read_bytes()) != notice["sha256"]:
                raise ValueError(f"Changed/missing notice: {notice['path']}")
    if (ROOT / "licenses/python-constraints.txt").read_text(encoding="utf-8") != constraints():
        raise ValueError("Python constraints do not match uv.lock")
    print(f"Verified notices for {len(observed)} locked dependency versions (offline).")


def refresh() -> None:
    DEST.mkdir(parents=True, exist_ok=True)
    with ThreadPoolExecutor(max_workers=6) as pool:
        rows = list(pool.map(collect, packages()))
    manifest = {"schema_version": 1, "input_hash_normalization": "CRLF-to-LF", "inputs": {p: input_digest(p) for p in LOCKS},
                "scope": "All locked dependency versions, including optional, test and target-specific packages; not all are shipped on every target.",
                "packages": rows}
    (DEST / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    (ROOT / "licenses/python-constraints.txt").write_text(constraints(), encoding="utf-8")
    links = ["# Dependency notices and source archives", "",
             "Each source archive was verified against its lockfile checksum before copying notices.",
             "The source links below also provide the corresponding MPL-covered source, including webpki-roots and certifi.",
             "These are third-party licenses; the repository MIT license does not replace them.", "",
             "| Ecosystem | Package | Source archive | Notices |", "| --- | --- | --- | --- |"]
    for row in rows:
        notices = ", ".join(f"[{Path(n['path']).name}]({Path(n['path']).relative_to('licenses').as_posix()})" for n in row["notices"])
        links.append(f"| {row['ecosystem']} | {row['name']} {row['version']} | [source]({row['source_url']}) | {notices} |")
    (ROOT / "licenses/README.md").write_text("\n".join(links) + "\n", encoding="utf-8")
    check()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--refresh", action="store_true")
    group.add_argument("--check", action="store_true")
    args = parser.parse_args()
    refresh() if args.refresh else check()
