#!/usr/bin/env python3
"""Regression coverage for scoped pnpm and Cargo command construction."""

from __future__ import annotations

import json
import os
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FRONTEND_RUNNER = ROOT / ".github" / "scripts" / "run-frontend-scope.sh"
RUST_RUNNER = ROOT / ".github" / "scripts" / "run-rust-scope.sh"
FAKE_COMMAND = """#!/usr/bin/env python3
import json
import os
import sys

with open(os.environ["COMMAND_LOG"], "a", encoding="utf-8") as output:
    output.write(json.dumps({"cwd": os.getcwd(), "argv": sys.argv[1:]}) + "\\n")
"""


with tempfile.TemporaryDirectory(prefix="devbox-ci-runner-") as temporary:
    temporary_path = Path(temporary)
    binary_path = temporary_path / "bin"
    binary_path.mkdir()
    log_path = temporary_path / "commands.jsonl"

    for command_name in ("cargo", "pnpm"):
        command_path = binary_path / command_name
        command_path.write_text(FAKE_COMMAND, encoding="utf-8")
        command_path.chmod(0o755)

    environment = os.environ.copy()
    environment["PATH"] = f"{binary_path}{os.pathsep}{environment['PATH']}"
    environment["COMMAND_LOG"] = str(log_path)

    def run(*arguments: str, succeeds: bool = True) -> list[dict[str, object]]:
        log_path.write_text("", encoding="utf-8")
        completed = subprocess.run(
            arguments,
            cwd=ROOT,
            env=environment,
            check=False,
            capture_output=True,
            text=True,
        )
        if succeeds and completed.returncode != 0:
            raise AssertionError(completed.stderr or completed.stdout)
        if not succeeds and completed.returncode == 0:
            raise AssertionError(f"command unexpectedly passed: {arguments!r}")
        return [json.loads(line) for line in log_path.read_text(encoding="utf-8").splitlines()]

    calls = run("bash", str(FRONTEND_RUNNER), "build", "all")
    assert calls == [{"cwd": str(ROOT), "argv": ["build"]}]

    calls = run(
        "bash",
        str(FRONTEND_RUNNER),
        "test",
        "apps",
        "apps/run-manager,packages/diff-view",
    )
    assert calls == [{
        "cwd": str(ROOT),
        "argv": [
            "-r",
            "--filter",
            "./apps/run-manager",
            "--filter",
            "./packages/diff-view",
            "test",
        ],
    }]

    calls = run(
        "bash",
        str(FRONTEND_RUNNER),
        "typecheck",
        "apps",
        "apps/code-pad,packages/editor",
    )
    assert calls == [{
        "cwd": str(ROOT / "packages" / "editor"),
        "argv": ["exec", "tsc", "--noEmit"],
    }]

    assert run("bash", str(FRONTEND_RUNNER), "build", "none") == []
    assert run("bash", str(FRONTEND_RUNNER), "build", "apps", "../outside", succeeds=False) == []

    calls = run(
        "bash",
        str(RUST_RUNNER),
        "check",
        "packages",
        "process,port-manager",
    )
    assert calls == [{
        "cwd": str(ROOT),
        "argv": ["check", "-p", "process", "-p", "port-manager"],
    }]

    calls = run(
        "bash",
        str(RUST_RUNNER),
        "clippy",
        "packages",
        "process,port-manager",
    )
    assert calls == [{
        "cwd": str(ROOT),
        "argv": [
            "clippy",
            "-p",
            "process",
            "-p",
            "port-manager",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    }]

    calls = run("bash", str(RUST_RUNNER), "fmt", "packages", "process")
    assert calls == [{"cwd": str(ROOT), "argv": ["fmt", "--all", "--check"]}]

    calls = run("bash", str(RUST_RUNNER), "test", "all")
    assert calls == [{"cwd": str(ROOT), "argv": ["test", "--workspace"]}]

    assert run("bash", str(RUST_RUNNER), "check", "none") == []
    assert run("bash", str(RUST_RUNNER), "check", "packages", "", succeeds=False) == []

print("CI scope runner regression tests passed")
