#!/usr/bin/env python3
"""Build the private v0.8 setup preparation entrypoint from verified package bytes.

This does not publish a candidate or certify activation/update/uninstall acceptance.
Actual compilation is Windows-only. --emit-only writes the reviewable NSIS input.
"""
import argparse
import importlib.util
import json
import os
import subprocess
from pathlib import Path

spec = importlib.util.spec_from_file_location("suite_package", Path(__file__).with_name("build-suite-package.py"))
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


def literal(value):
    value = str(value)
    if any(char in value for char in ("\0", "\r", "\n")):
        raise ValueError("invalid NSIS literal")
    return '"' + value.replace("$", "$$").replace('"', '$\\"') + '"'


def prepare(staging, bootstrap):
    payload_path = staging / "suite-payload.json"
    if payload_path.is_symlink() or payload_path.stat().st_size > 256 * 1024:
        raise ValueError("invalid private payload")
    payload = json.loads(payload_path.read_text(encoding="utf-8"))
    version = payload["suiteVersion"]
    package.identity(version, payload["sourceSha"])
    if payload["schemaVersion"] != 1 or payload["protocolVersion"] != 1 or [p["id"] for p in payload["products"]] != list(package.PRODUCTS):
        raise ValueError("unexpected private package topology")
    inputs = [(payload_path, package.asset(payload_path))]
    for product in payload["products"]:
        expected = product["portable"]
        if product["version"] != version or expected["name"] != f'devbox-{product["id"]}_{version}_x64.zip':
            raise ValueError("unexpected product archive")
        source = staging / expected["name"]
        if package.asset(source) != expected:
            raise ValueError("package changed before installer assembly")
        inputs.append((source, expected))
    helper = next(file for product in payload["products"] if product["id"] == "control-center"
                  for file in product["files"] if file["name"] == "resources/suite/devbox-suite-bootstrap.exe")
    if package.asset(bootstrap, helper["name"]) != helper:
        raise ValueError("bootstrap differs from reviewed Control Center package")
    inputs.append((bootstrap, helper))
    output = staging / f"Devbox_{version}_x64-setup.exe"
    script = staging / "suite-setup.nsi"
    if output.exists() or script.exists():
        raise ValueError("installer output already exists")
    files = "\n".join(f"  File {literal('/oname=' + name)} {literal(path.resolve())}" for path, name in [
        (payload_path, "suite-payload.json"), (bootstrap, "devbox-suite-bootstrap.exe"),
        *[(path, asset["name"]) for path, asset in inputs[1:-1]],
    ])
    source = r'''; Generated from the exact private payload. No legacy uninstaller is invoked.
Unicode true
ManifestDPIAware true
RequestExecutionLevel user
!include "MUI2.nsh"
!include "LogicLib.nsh"
Name "Devbox __VERSION__"
OutFile __OUTPUT__
InstallDir "$LOCALAPPDATA\DevboxSuite"
SetCompressor /SOLID lzma
!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TEXT "Workspace, API Studio, Knowledge, Control Center를 준비합니다. 기존 앱 데이터는 Control Center에서 검토한 뒤 이전합니다."
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_TITLE "Devbox 설치 준비 완료"
!define MUI_FINISHPAGE_TEXT "Control Center에서 데이터 이전과 활성화를 완료하세요. 기존 앱과 원본 데이터는 유지됩니다."
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_TEXT "Control Center에서 계속"
!define MUI_FINISHPAGE_RUN_FUNCTION OpenControlCenter
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_LANGUAGE "Korean"
Section "Suite 준비"
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
__PAYLOAD_FILES__
  nsExec::ExecToStack '"$PLUGINSDIR\devbox-suite-bootstrap.exe" --prepare-install "$INSTDIR" "$PLUGINSDIR\suite-payload.json"'
  Pop $0
  Pop $1
  ${If} $0 != 0
    DetailPrint $1
    SetErrorLevel 1
    Abort "설치를 준비하지 못했습니다. 기존 파일과 데이터는 보존됩니다."
  ${EndIf}
SectionEnd
Function OpenControlCenter
  nsExec::ExecToStack '"$PLUGINSDIR\devbox-suite-bootstrap.exe" --open-install "$INSTDIR" "$PLUGINSDIR\suite-payload.json"'
  Pop $0
  Pop $1
  ${If} $0 != 0
    MessageBox MB_OK|MB_ICONEXCLAMATION "Control Center를 열지 못했습니다. 준비한 설치 파일과 데이터는 보존됩니다."
  ${EndIf}
FunctionEnd
'''.replace("__VERSION__", version).replace("__OUTPUT__", literal(output.resolve())).replace("__PAYLOAD_FILES__", files)
    with script.open("x", encoding="utf-8-sig", newline="\n") as target:
        target.write(source)
    return script, output, inputs


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("staging", type=Path)
    parser.add_argument("bootstrap", type=Path)
    parser.add_argument("--makensis", type=Path)
    parser.add_argument("--emit-only", action="store_true")
    args = parser.parse_args()
    if not args.emit_only and (os.name != "nt" or not args.makensis or not args.makensis.is_file()):
        raise ValueError("installer compilation requires an explicit Windows NSIS compiler")
    script, output, inputs = prepare(args.staging, args.bootstrap)
    if args.emit_only:
        return
    subprocess.run([str(args.makensis.resolve()), "/V2", str(script.resolve())], check=True, timeout=600)
    for path, expected in inputs:
        if package.asset(path, expected["name"]) != expected:
            raise ValueError("private input changed during installer assembly")
    package.asset(output)


if __name__ == "__main__":
    main()
