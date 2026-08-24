#!/usr/bin/env python3
"""Validate dependency policy and generate deterministic third-party notices."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
import tomllib
from datetime import date
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
POLICY_PATH = ROOT / ".github" / "dependency-policy.json"
DENY_PATH = ROOT / "deny.toml"
NOTICES_PATH = ROOT / "THIRD_PARTY_NOTICES.md"


def fail(message: str) -> None:
    raise SystemExit(f"dependency policy failed: {message}")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run_json(command: list[str]) -> Any:
    completed = subprocess.run(
        command,
        cwd=ROOT,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        return json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        fail(f"invalid JSON from {' '.join(command)}: {error}")


def load_policy() -> dict[str, Any]:
    policy = json.loads(POLICY_PATH.read_text(encoding="utf-8"))
    if policy.get("schemaVersion") != 1:
        fail("unsupported dependency policy schema")
    return policy


def cargo_lock_packages() -> list[dict[str, Any]]:
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
    return lock.get("package", [])


def validate_exceptions(policy: dict[str, Any], cargo_packages: list[dict[str, Any]]) -> None:
    exceptions = policy.get("advisoryExceptions", [])
    ids = [entry.get("id") for entry in exceptions]
    if len(ids) != len(set(ids)):
        fail("advisory exception IDs must be unique")

    locked = {(entry["name"], entry["version"]) for entry in cargo_packages}
    today = date.today()
    for entry in exceptions:
        required = ("id", "package", "version", "detector", "scope", "expires", "reason")
        if any(not entry.get(field) for field in required):
            fail(f"incomplete advisory exception: {entry!r}")
        if (entry["package"], entry["version"]) not in locked:
            fail(f"advisory exception package is not locked: {entry['package']}@{entry['version']}")
        try:
            expires = date.fromisoformat(entry["expires"])
        except ValueError:
            fail(f"invalid exception expiry for {entry['id']}: {entry['expires']}")
        if expires <= today:
            fail(f"expired advisory exception {entry['id']} ({entry['expires']})")
        if entry["detector"] not in {"cargo-deny", "dependabot"}:
            fail(f"unknown advisory detector for {entry['id']}")

    deny = tomllib.loads(DENY_PATH.read_text(encoding="utf-8"))
    deny_entries = deny.get("advisories", {}).get("ignore", [])
    deny_ids: set[str] = set()
    for entry in deny_entries:
        if not isinstance(entry, dict) or not entry.get("id") or not entry.get("reason"):
            fail("every cargo-deny advisory ignore must include id and reason")
        deny_ids.add(entry["id"])
        matching = next((item for item in exceptions if item["id"] == entry["id"]), None)
        if matching is None:
            fail(f"cargo-deny ignore lacks policy metadata: {entry['id']}")
        if matching["expires"] not in entry["reason"]:
            fail(f"cargo-deny reason lacks expiry date: {entry['id']}")

    policy_deny_ids = {
        entry["id"] for entry in exceptions if entry["detector"] == "cargo-deny"
    }
    if deny_ids != policy_deny_ids:
        missing = sorted(policy_deny_ids - deny_ids)
        extra = sorted(deny_ids - policy_deny_ids)
        fail(f"cargo-deny exception mismatch; missing={missing}, extra={extra}")


def parse_pnpm_integrities() -> dict[tuple[str, str], str]:
    text = (ROOT / "pnpm-lock.yaml").read_text(encoding="utf-8")
    integrities: dict[tuple[str, str], str] = {}
    in_packages = False
    current: tuple[str, str] | None = None

    for line in text.splitlines():
        if line == "packages:":
            in_packages = True
            continue
        if line == "snapshots:":
            break
        if not in_packages:
            continue

        key_match = re.fullmatch(r"  ([^ ].*):", line)
        if key_match:
            raw = key_match.group(1)
            if len(raw) >= 2 and raw[0] == raw[-1] and raw[0] in {"'", '"'}:
                raw = raw[1:-1]
            if "@" not in raw:
                fail(f"cannot parse pnpm package key: {raw}")
            name, version = raw.rsplit("@", 1)
            current = (name, version)
            continue

        integrity_match = re.search(r"\bintegrity: ([^,}]+)", line)
        if current and integrity_match:
            integrity = integrity_match.group(1).strip()
            if current in integrities and integrities[current] != integrity:
                fail(f"conflicting pnpm integrity for {current[0]}@{current[1]}")
            integrities[current] = integrity

    if not integrities:
        fail("pnpm lockfile package integrity table is empty")
    return integrities


def flatten_pnpm_licenses(
    inventory: dict[str, list[dict[str, Any]]],
    policy: dict[str, Any],
    integrities: dict[tuple[str, str], str],
) -> list[dict[str, str]]:
    allowed = set(policy.get("allowedPnpmLicenses", []))
    clarifications: dict[tuple[str, str, str], dict[str, Any]] = {}
    for entry in policy.get("licenseClarifications", []):
        required = (
            "package",
            "version",
            "reportedLicense",
            "acceptedLicense",
            "source",
            "integrity",
            "reason",
        )
        if any(not entry.get(field) for field in required):
            fail(f"incomplete license clarification: {entry!r}")
        if entry["acceptedLicense"] not in allowed:
            fail(
                "license clarification resolves outside the allowlist: "
                f"{entry['package']}@{entry['version']} -> {entry['acceptedLicense']}"
            )
        key = (entry["package"], entry["version"], entry["reportedLicense"])
        if key in clarifications:
            fail(f"duplicate license clarification: {key!r}")
        clarifications[key] = entry
    used_clarifications: set[tuple[str, str, str]] = set()
    rows: list[dict[str, str]] = []

    for reported_license, packages in inventory.items():
        if reported_license not in allowed and reported_license != "Unknown":
            fail(f"unapproved pnpm license expression: {reported_license}")
        for package in packages:
            name = package.get("name")
            versions = package.get("versions") or []
            if not name or not versions:
                fail(f"incomplete pnpm license entry: {package!r}")
            for version in versions:
                integrity = integrities.get((name, version))
                if integrity is None:
                    fail(f"pnpm package lacks locked integrity: {name}@{version}")
                accepted_license = reported_license
                source = package.get("homepage") or "npm registry"
                if reported_license == "Unknown":
                    clarification = clarifications.get((name, version, reported_license))
                    if clarification is None:
                        fail(f"unknown pnpm license without clarification: {name}@{version}")
                    if clarification["integrity"] != integrity:
                        fail(f"license clarification integrity mismatch: {name}@{version}")
                    used_clarifications.add((name, version, reported_license))
                    accepted_license = clarification["acceptedLicense"]
                    source = clarification["source"]
                rows.append(
                    {
                        "name": name,
                        "version": version,
                        "license": accepted_license,
                        "source": source,
                        "integrity": integrity,
                    }
                )

    stale_clarifications = sorted(set(clarifications) - used_clarifications)
    if stale_clarifications:
        fail(f"unused license clarifications: {stale_clarifications!r}")

    rows.sort(key=lambda row: (row["name"].lower(), row["version"]))
    return rows


def rust_rows(cargo_packages: list[dict[str, Any]]) -> list[dict[str, str]]:
    metadata = run_json(["cargo", "metadata", "--locked", "--format-version", "1"])
    locked = {
        (entry["name"], entry["version"], entry.get("source")): entry
        for entry in cargo_packages
    }
    rows: list[dict[str, str]] = []
    for package in metadata.get("packages", []):
        source = package.get("source")
        if source is None:
            continue
        license_expression = package.get("license")
        if not license_expression:
            fail(f"external Cargo package has no license expression: {package['name']}@{package['version']}")
        lock = locked.get((package["name"], package["version"], source))
        if lock is None or not lock.get("checksum"):
            fail(f"external Cargo package lacks locked checksum: {package['name']}@{package['version']}")
        rows.append(
            {
                "name": package["name"],
                "version": package["version"],
                "license": license_expression,
                "source": package.get("repository") or f"https://crates.io/crates/{package['name']}",
                "integrity": f"sha256:{lock['checksum']}",
            }
        )
    rows.sort(key=lambda row: (row["name"].lower(), row["version"]))
    return rows


def escape_cell(value: str) -> str:
    return value.replace("|", "\\|").replace("\n", " ")


def render_notices(rust: list[dict[str, str]], pnpm: list[dict[str, str]]) -> str:
    cargo_digest = sha256(ROOT / "Cargo.lock")
    pnpm_digest = sha256(ROOT / "pnpm-lock.yaml")
    lines = [
        "# Third-Party Notices",
        "",
        "This inventory is generated from the locked devbox dependency graph. It does not grant a",
        "license for devbox itself; workspace packages are private and excluded from this third-party",
        "inventory. Regenerate it with `.github/scripts/check-dependencies.py generate`.",
        "",
        f"- Cargo.lock SHA-256: `{cargo_digest}`",
        f"- pnpm-lock.yaml SHA-256: `{pnpm_digest}`",
        "",
        "## Rust dependencies",
        "",
        "| Package | Version | License | Source | Locked digest |",
        "|---|---:|---|---|---|",
    ]
    for row in rust:
        lines.append(
            "| {name} | {version} | {license} | {source} | `{integrity}` |".format(
                **{key: escape_cell(value) for key, value in row.items()}
            )
        )

    lines.extend(
        [
            "",
            "## Frontend runtime dependencies",
            "",
            "The frontend table uses `pnpm licenses list --prod`; build-only packages remain subject",
            "to the CI license gate but are not shipped in the compiled frontend bundle.",
            "",
            "| Package | Version | License | Source | Locked integrity |",
            "|---|---:|---|---|---|",
        ]
    )
    for row in pnpm:
        lines.append(
            "| {name} | {version} | {license} | {source} | `{integrity}` |".format(
                **{key: escape_cell(value) for key, value in row.items()}
            )
        )
    lines.append("")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("check", "generate"))
    args = parser.parse_args()

    policy = load_policy()
    cargo_packages = cargo_lock_packages()
    validate_exceptions(policy, cargo_packages)
    integrities = parse_pnpm_integrities()

    # The full inventory gates build and test dependencies too.
    flatten_pnpm_licenses(
        run_json(["pnpm", "licenses", "list", "--json"]),
        policy,
        integrities,
    )
    runtime_rows = flatten_pnpm_licenses(
        run_json(["pnpm", "licenses", "list", "--prod", "--json"]),
        policy,
        integrities,
    )
    notices = render_notices(rust_rows(cargo_packages), runtime_rows)

    if args.mode == "generate":
        NOTICES_PATH.write_text(notices, encoding="utf-8", newline="\n")
        print(f"generated {NOTICES_PATH.relative_to(ROOT)}")
        return

    if not NOTICES_PATH.exists():
        fail("THIRD_PARTY_NOTICES.md is missing")
    if NOTICES_PATH.read_text(encoding="utf-8") != notices:
        fail("THIRD_PARTY_NOTICES.md is stale; run check-dependencies.py generate")
    print("dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        details = error.stderr.strip() or error.stdout.strip() or str(error)
        fail(details)
