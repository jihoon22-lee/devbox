#!/usr/bin/env python3
"""Assemble v0.8 product ZIPs, then bind the completed setup to seven public assets.

Input is a private, already-built product payload. This script neither builds nor
executes an app and does not promote a release candidate.
"""
import argparse
import hashlib
import json
import re
import stat
import zipfile
from pathlib import Path

PRODUCTS = ("workspace", "api-studio", "knowledge", "control-center")
MAX_BYTES = 1024 * 1024 * 1024


def asset(path, name=None):
    if path.is_symlink() or not path.is_file() or not 0 < path.stat().st_size <= MAX_BYTES:
        raise ValueError(f"invalid payload file: {path.name}")
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(65536), b""):
            digest.update(chunk)
    return {"name": name or path.name, "sha256": digest.hexdigest(), "size": path.stat().st_size}


def write_json(path, value):
    with path.open("x", encoding="utf-8", newline="\n") as output:
        json.dump(value, output, ensure_ascii=False, indent=2)
        output.write("\n")


def identity(version, source):
    if not re.fullmatch(r"(?:0|[1-9][0-9]{0,7})(?:\.(?:0|[1-9][0-9]{0,7})){2}", version):
        raise ValueError("invalid suite version")
    if not re.fullmatch(r"[a-f0-9]{40}", source):
        raise ValueError("exact source SHA required")


def portables(payload, output, version, source):
    identity(version, source)
    output.mkdir()  # A prior candidate or interrupted stage is never overwritten.
    notices = payload / "THIRD_PARTY_NOTICES.md"
    notice = asset(notices)
    (output / notice["name"]).write_bytes(notices.read_bytes())
    products = []
    for product in PRODUCTS:
        root = payload / product
        names = [f"devbox-{product}.exe"]
        if product == "workspace":
            names += ["resources/wsl/manifest.json", "resources/wsl/devbox-workspace-wsl"]
        if product == "control-center":
            names += ["resources/suite/devbox-suite-bootstrap.exe"]
        files = [asset(root / name, name) for name in names]
        executable = files[0]
        portable_manifest = {
            "schemaVersion": 1, "installationId": "portable", "generation": f"portable-{source}",
            "suiteVersion": version, "protocolVersion": 1,
            "members": [{"product": product, "executable": executable["name"], "sha256": executable["sha256"]}],
        }
        portable_bytes = (json.dumps(portable_manifest, indent=2) + "\n").encode()
        files += [notice, {"name": "devbox-installation.json", "sha256": hashlib.sha256(portable_bytes).hexdigest(), "size": len(portable_bytes)}]
        if sum(file["size"] for file in files) > MAX_BYTES:
            raise ValueError("unpacked product exceeds limit")
        archive = output / f"devbox-{product}_{version}_x64.zip"
        # Fixed entry order/timestamps: retrying assembly cannot change ZIP bytes.
        with zipfile.ZipFile(archive, "x", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as target:
            for file in sorted(files, key=lambda file: file["name"]):
                name = file["name"]
                info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = (stat.S_IFREG | 0o644) << 16
                content = portable_bytes if name == "devbox-installation.json" else (notices if name == notice["name"] else root / name).read_bytes()
                if len(content) != file["size"] or hashlib.sha256(content).hexdigest() != file["sha256"]:
                    raise ValueError("payload changed during assembly")
                target.writestr(info, content, compresslevel=9)
        products.append({"id": product, "version": version, "portable": asset(archive), "files": files})
    # Private installer input; it is excluded from public assets.
    write_json(output / "suite-payload.json", {"schemaVersion": 1, "suiteVersion": version, "sourceSha": source, "protocolVersion": 1, "products": products, "notices": notice})


def manifest(staging, output):
    payload = json.loads((staging / "suite-payload.json").read_text())
    identity(payload["suiteVersion"], payload["sourceSha"])
    if payload["schemaVersion"] != 1 or [p["id"] for p in payload["products"]] != list(PRODUCTS):
        raise ValueError("private suite payload identity mismatch")
    version = payload["suiteVersion"]
    for product in payload["products"]:
        if asset(staging / product["portable"]["name"]) != product["portable"]:
            raise ValueError("portable changed after installer assembly")
    if asset(staging / "THIRD_PARTY_NOTICES.md") != payload["notices"]:
        raise ValueError("notices changed after installer assembly")
    setup = asset(staging / f"Devbox_{version}_x64-setup.exe")
    write_json(output, {"schemaVersion": 2, "releaseTag": f"v{version}", "sourceSha": payload["sourceSha"], "suiteVersion": version, "protocolVersion": 1, "setup": setup, "products": payload["products"], "notices": payload["notices"]})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="mode", required=True)
    packages = commands.add_parser("portables")
    packages.add_argument("payload", type=Path)
    packages.add_argument("output", type=Path)
    packages.add_argument("version")
    packages.add_argument("source")
    final = commands.add_parser("manifest")
    final.add_argument("staging", type=Path)
    final.add_argument("output", type=Path)
    args = parser.parse_args()
    if args.mode == "portables":
        portables(args.payload, args.output, args.version, args.source)
    else:
        manifest(args.staging, args.output)


if __name__ == "__main__":
    main()
