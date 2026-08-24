# Rust 1.98 Clippy Compatibility

## Overview

GitHub Actions의 stable Rust가 1.98.0으로 갱신되면서 새
`clippy::chunks-exact-to-as-chunks` lint가 `-D warnings` 아래에서 기존 빌드를
차단했다. PR #226에서 저장소의 상수 크기 `chunks_exact` 사용 세 곳을 모두
`as_chunks`로 전환하고 기존 디코딩·해시 파싱 동작을 유지했다.

## Context

- API Playground 보안 핫픽스 PR #225의 Linux 및 Windows Rust CI가 새 lint로
  실패하면서 toolchain 변화가 처음 확인됐다.
- 첫 실패는 WSL UTF-16LE 출력 디코더에서 발생했고, 이를 수정한 뒤 Rust 1.98이
  Code Pad의 UTF-16 디코더와 SHA-256 파서에서도 같은 lint를 검출했다.
- 저장소 전체 Rust 소스를 검색해 상수 인자를 사용하는 `chunks_exact` 호출이
  이 세 곳뿐임을 확인한 후 하나의 toolchain 호환성 변경으로 처리했다.

## Changes Made

### WSL output decoding

- 파일: `crates/wsl/src/output.rs`
- 2바이트 청크 변환을 `as_chunks::<2>().0.iter()`로 변경했다.
- 기존 `chunks_exact(2)`와 동일하게 불완전한 마지막 바이트를 무시하도록
  유지하고, 이 계약을 `ignores_incomplete_utf16le_trailing_byte` 테스트로
  고정했다.

### Code Pad UTF-16 decoding

- 파일: `apps/code-pad/src-tauri/src/core/encoding.rs`
- 길이가 짝수인지 먼저 검증하는 기존 경계를 유지한 채 2바이트 배열을 직접
  `u16::from_le_bytes` 또는 `u16::from_be_bytes`에 전달하도록 변경했다.
- 잘못된 홀수 길이와 잘못된 BOM은 기존과 같이 변환 전에 거부된다.

### Code Pad SHA-256 parsing

- 파일: `apps/code-pad/src-tauri/src/lsp/installer.rs`
- 정확히 64자인지 먼저 확인하는 기존 검증 뒤에 `as_chunks::<2>()`로 32개
  hex 바이트를 파싱하도록 변경했다.
- 허용되는 소문자 hex와 잘못된 문자 거부 동작에는 변화가 없다.

## Code Examples

이전 상수 크기 iterator:

```rust
bytes.chunks_exact(2).map(|chunk| {
    u16::from_le_bytes([chunk[0], chunk[1]])
})
```

Rust 1.98 권장 형태:

```rust
bytes
    .as_chunks::<2>()
    .0
    .iter()
    .map(|chunk| u16::from_le_bytes(*chunk))
```

## Verification Results

로컬 검증:

```text
cargo fmt --all -- --check                                  PASS
cargo clippy -p wsl -p code-pad --all-targets -- -D warnings PASS
cargo test -p wsl -p code-pad --all-targets                 PASS
cargo test --workspace --all-targets                        PASS
cargo check --workspace                                     PASS
pnpm -r --workspace-concurrency=1 build                     PASS
repository-wide constant chunks_exact search                0 matches
git diff --check                                            PASS
```

GitHub Actions PR #226:

```text
Catalog consistency          PASS
Detect changed scope         PASS
Frontend (pnpm)              PASS
Rust (Cargo workspace)       PASS
Rust (Windows compile check) PASS
```

PR #226은 모든 필수 CI가 통과한 뒤 2026-08-24에 squash merge됐다.

## Follow-up

- API Playground 보안 핫픽스 PR #225를 이 변경이 포함된 최신 `main` 위에
  재배치해 Linux와 Windows CI를 다시 실행한다.
- stable toolchain을 계속 추적하므로 앞으로 새 lint가 활성화될 때에도 기능
  변경과 toolchain 호환성 변경을 분리해 원인을 명확히 유지한다.
