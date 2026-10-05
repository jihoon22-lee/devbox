"""Independent, fail-closed packaged UI evidence checks for candidate promotion."""
import hashlib
import json
from pathlib import Path
import re
from candidate_revalidation import validate_revalidation_proof


def reject_diagnostic(value):
    if any(key in value for key in ("diagnosticOnly", "runnerSourceSha", "runnerRunId", "payloadSourceSha", "payloadRunId")) or value.get("promotionEvidence") is False:
        raise ValueError("diagnostic evidence cannot be promoted")


def verify_user_flow_evidence(file: Path, commit: str, assets: list[dict], revalidation_proof=None) -> dict:
    if file.is_symlink() or not file.is_file():
        raise ValueError("user-flow evidence file missing")
    evidence = json.loads(file.read_text(encoding="utf-8"))
    reject_diagnostic(evidence)
    fixture = commit
    if revalidation_proof is not None:
        fixture = validate_revalidation_proof(revalidation_proof, commit, assets=assets)["fixtureSha"]
    matrix = json.loads((Path(__file__).parent / "suite-user-flow-matrix.json").read_text(encoding="utf-8"))
    expected = {item["name"]: item["digest"].removeprefix("sha256:") for item in assets}
    if evidence.get("schemaVersion") != 1 or evidence.get("expectedSource") != commit or evidence.get("expectedFixture") != fixture or evidence.get("expectedDigests") != expected:
        raise ValueError("user-flow package identity mismatch")
    results, screenshots = evidence.get("results"), evidence.get("screenshots")
    if not isinstance(results, list) or not isinstance(screenshots, dict) or len(results) != len(matrix):
        raise ValueError("user-flow result set incomplete")
    if len({row.get("id") for row in results}) != len(matrix) or {row.get("id") for row in results} != {row["id"] for row in matrix}:
        raise ValueError("user-flow required scenarios mismatch")
    installation_key = evidence.get("installationKey", "")
    if not re.fullmatch(r"[a-f0-9]{64}", installation_key):
        raise ValueError("interactive installation identity missing")
    for required in matrix:
        row = next(row for row in results if row["id"] == required["id"])
        reject_diagnostic(row)
        assertions, paths = row.get("assertions"), row.get("screenshotPaths")
        same_installation = row.get("installationKey") == installation_key
        legacy_child = row.get("id") == "DELIVERY-01" and row.get("fixtureKind") == "legacy-upgrade" and row.get("parentInstallationKey") == installation_key and re.fullmatch(r"[a-f0-9]{64}", row.get("installationKey", ""))
        if not (same_installation or legacy_child) or row.get("status") != "PASS" or row.get("sourceSha") != commit or row.get("fixtureSha") != fixture or row.get("artifactDigests") != expected or row.get("evidenceKind") != required["evidenceKind"] or row.get("failureCode") is not None:
            raise ValueError(f"user-flow scenario rejected: {required['id']}")
        if not isinstance(assertions, list) or not assertions or not all(isinstance(a, str) and a.strip() for a in assertions) or not isinstance(paths, list) or not paths:
            raise ValueError("user-flow observations missing")
        for name in paths:
            if not isinstance(name, str) or "\\" in name or Path(name).is_absolute() or ".." in Path(name).parts or not re.fullmatch(r"[a-f0-9]{64}", screenshots.get(name, "")):
                raise ValueError("user-flow screenshot identity invalid")
            image = file.parent / "user-flow-files" / name
            if image.is_symlink() or not image.is_file():
                raise ValueError("user-flow screenshot missing")
            content = image.read_bytes()
            if not content.startswith(b"\x89PNG\r\n\x1a\n") or hashlib.sha256(content).hexdigest() != screenshots[name]:
                raise ValueError("user-flow screenshot changed")
    return {"status": "PASS", "sourceSha": commit, "requiredScenarios": len(matrix), "sha256": hashlib.sha256(file.read_bytes()).hexdigest(), "file": file.name}
