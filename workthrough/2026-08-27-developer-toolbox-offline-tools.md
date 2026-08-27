# Developer Toolbox offline tools (#289–#292)

## Summary

Developer Toolbox 0.3.0에 JWT decode/verify, Lorem generator, Markdown table formatter,
QR generator를 하나의 cohesive offline-tools 기능 경계로 통합했다. 네 이슈는 모두 사용자가
외부 사이트나 별도 실행 파일로 텍스트·토큰·표·QR 데이터를 옮기지 않고 같은 앱 안에서 처리하게
한다. 공용 bounded input, explicit copy/save, stale-result/single-flight, fixed error, 접근성 계약을
공유하므로 하나의 PR에서 검증하되 각 이슈의 parser/crypto/renderer acceptance와 테스트는
독립적으로 유지한다.

## Included issues and acceptance

### #289 — JWT decode and verify

- compact JWT의 canonical base64url header/payload를 strict JSON bounds 안에서 표시한다.
- HS256/HS384/HS512만 allow-list하고 RustCrypto constant-time MAC 검증을 사용한다.
- algorithm confusion, duplicate/critical header, key/signature/JSON bounds를 fail-closed 처리한다.
- `exp`, `nbf`, `iat`를 native/browser direct verification 경계 모두에서 ±60초 clock skew로
  검증한다.
- token/key/signature는 저장·로그·자동 clipboard·결과 payload로 반환하지 않는다.

### #290 — Lorem generator

- paragraph/sentence/word 단위, bounded count, deterministic fixture를 제공한다.
- 생성은 명시적 action이며 copy와 UTF-8 text save도 명시적으로만 실행한다.
- 중복 action, late clipboard/save response, unmount 뒤 state update를 차단한다.

### #291 — Markdown table formatter

- escaped pipe, matched code span, alignment, uneven row를 bounded linear scan으로 처리한다.
- malformed separator, row/column/cell/input/output bounds는 raw input을 반사하지 않는 고정 오류로
  종료한다.
- queued formatting은 취소 가능하고 실행된 bounded core의 stale 결과는 sequence guard로 버린다.

### #292 — QR generator

- text, HTTP(S) URL, Wi-Fi preset을 완전 오프라인 SVG/PNG로 생성한다.
- native `qrcode`/`png`와 browser-preview `qrcode-generator`는 같은 payload validation,
  UTF-8 bounds, error-correction allow-list, 4 MiB binary/base64 결과 상한을 적용한다.
- SVG/PNG copy와 save는 explicit action이며 binary clipboard 미지원 환경은 고정 안내 후 PNG
  save로 유도한다.
- 카메라 decode, remote/dynamic QR, history와 자동 clipboard는 포함하지 않는다.

## Shared implementation boundary

- `src/tools/common.tsx`가 bounded paste, fixed clipboard/action error, mounted/revision/busy guard,
  binary download를 공용 제공한다.
- 각 도구는 UTF-16 code-unit가 아니라 UTF-8 byte 상한을 command/browser 경계와 UI에서 동일하게
  검사한다.
- raw input, JWT key/signature, native error detail은 사용자 오류·context menu·clipboard 오류에
  반사하지 않는다.
- production app은 native command를 우선 사용하고 browser preview는 같은 bounded contract를
  로컬에서 재현한다. QR encoder처럼 구현 라이브러리가 다른 경우 matrix byte equality가 아니라
  validation/metadata/error/determinism parity를 계약으로 삼는다.
- 네 도구는 서로 독립적으로 rollback 가능한 내부 모듈과 fixture를 유지하지만, 앱 버전·공용 UI
  변경·offline release 검증은 하나의 사용자 기능 경계로 배포한다.

## Root review follow-up

- JWT native verify가 서명만 확인하던 누락을 보완해 payload temporal claims도 command 경계에서
  다시 검증했다. direct browser verifier에도 같은 검사를 추가했다.
- JWT token/key UI guard를 UTF-8 byte 기준으로 바꾸고 multibyte overflow 및 고정 오류 회귀
  fixture를 추가했다.
- Markdown code-span parser를 indexed run chain으로 바꿔 matched backtick 탐색의 선형성을
  유지하고 unmatched backtick이 pipe delimiter를 숨기지 않게 했다.
- synchronous formatter를 microtask라고 잘못 설명하던 계약을 event-loop task scheduling과
  cooperative stale-result discard로 바로잡았다.
- QR의 generation/action single-flight, unmount/stale response, native/browser URL Unicode
  whitespace 및 PNG 결과 상한 parity를 보강했다.
- QR native request DTO에서 secret-bearing payload의 `Debug`/`Clone` 파생을 제거하고 unknown
  IPC field를 거부하는 strict serde fixture를 추가했다.
- dependency notices를 lockfile에서 재생성하고 pinned Rust/JavaScript QR 라이브러리의 정책
  일치를 확인했다.

## Verification

Linux/WSL에서 shared Cargo target과 최대 2 workers를 사용했다.

```text
cargo test -p developer-toolbox -j2                       PASS (40 tests)
cargo check -p developer-toolbox -j2                      PASS
cargo fmt --all -- --check                                PASS
pnpm --filter developer-toolbox test -- --maxWorkers=2    PASS (24 files, 197 tests)
pnpm --filter developer-toolbox build                     PASS (158 modules)
python3 .github/scripts/check-dependencies.py check       PASS
git diff --check                                           PASS
```

Focused integration gate는 JWT/Lorem/Markdown/QR/common 9개 파일의 68개 테스트도 별도로 통과했다.
Windows packaged W2 smoke와 GitHub Actions 6-check gate는 PR 단계에서 수행한다.

## Remaining release checks

- Windows packaged Tauri에서 native JWT/QR command와 browser-preview parity를 확인한다.
- offline cold start, IME/keyboard/focus, narrow layout, text/PNG save, clipboard capability failure를
  smoke-test한다.
- token/key/payload가 persistence, log, automatic clipboard 또는 raw error에 남지 않는지 release
  evidence를 기록한다.
- CI 전체 gate가 통과하기 전에는 main에 병합하지 않는다.
