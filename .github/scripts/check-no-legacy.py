#!/usr/bin/env python3
"""Fail when v0.7 import/cleanup code remains in a cleaned scope."""
from __future__ import annotations
import argparse, pathlib, re, sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCOPES: dict[str, list[str]] = {
    "workspace": ["apps/devbox-workspace", "packages/workspace-features", "crates/runtime-engine"],
    "knowledge-api-studio": ["apps/devbox-knowledge", "apps/devbox-api-studio", "packages/knowledge-features", "packages/api-studio-features", "crates/http-client-engine"],
    "control-center": [
        "apps/devbox-control-center",
        "crates/installation-tools",
        "packages/control-center-features",
    ],
}
PATTERNS = [
    r"\blegacy_(inventory|snapshot|workspace|recovery|references|imports)\b",
    r"\b(migration_ledger|(?:window|terminal|recovery)_import|settings_import|runtime_import|prepare_legacy_snapshot|resolve_legacy_reference|MigrationOnly|legacySources|imported_log_descriptor)\b",
    r"\bpreview_(session|profile|template|lsp_config)_import\b",
    r"\bLegacy(Imports|LspImport|ProfileImport|RecoveryImport|ReferenceLookup|SessionImport|TemplateImport|WindowImport|Workspace)\b",

    r"com\.devbox\.(knowledgebase|lifelog|everythingplus|apiplayground|webhooklab|developertoolbox)\b",
    r"\b(MigrationStartup|MigrationSetup|import_plan|import_rows|list_import_sources|prepare_migration|migration_export|legacy_profile)\b",
    r"legacy[-_]v0\.7", r"legacy-v0\.7-catalog", r"\blegacy_cleanup\b", r"\blauncher_import\b",
    r"\bLegacyCleanup\b", r"\bLegacyInventory\b", r"\bLauncherImport\b", r"\bMigrationOwners\b",
    r"\bcutover_review\b", r"\bprepare_cutover\b", r"\brecord_migration_owner\b",
    r"\blegacy_installations\b", r"\bcleanup_legacy_portable\b", r"\binspect_data_databases\b",
    r"\binspect_local_quality\b", r"com\.devbox\.devboxmanager",
]
SUFFIXES = {".rs", ".ts", ".tsx", ".json", ".toml", ".mjs", ".ps1", ".py"}

def scan(paths: list[str]) -> list[str]:
    compiled = [re.compile(pattern) for pattern in PATTERNS]
    hits = []
    for relative in paths:
        base = ROOT / relative
        for path in ([base] if base.is_file() else base.rglob("*")):
            # This module only decodes/validates retained v0.8.1 journal DTOs.
            # It has no filesystem acquisition, launcher or import entrypoints.
            if path.relative_to(ROOT).as_posix() == "apps/devbox-control-center/src-tauri/src/core/owner_history.rs":
                continue
            if not path.is_file() or path.suffix not in SUFFIXES or "node_modules" in path.parts or "target" in path.parts:
                continue
            for number, line in enumerate(path.read_text(encoding="utf-8", errors="ignore").splitlines(), 1):
                if any(pattern.search(line) for pattern in compiled):
                    hits.append(f"{path.relative_to(ROOT)}:{number}: {line.strip()[:120]}")
    return hits

def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--scope", default="all", choices=[*SCOPES, "all"])
    args = parser.parse_args()
    paths = [p for name, values in SCOPES.items() if args.scope in (name, "all") for p in values]
    hits = scan(paths)
    print("\n".join(hits) if hits else f"no v0.7 legacy code in scope {args.scope}")
    return 1 if hits else 0

if __name__ == "__main__":
    sys.exit(main())
