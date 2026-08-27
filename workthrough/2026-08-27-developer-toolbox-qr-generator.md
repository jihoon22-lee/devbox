# Developer Toolbox QR Generator 구현

> 2026-08-27 PR 경계 갱신: #292 acceptance는 사용자 결정에 따라 #289–#291과 같은 Developer
> Toolbox 0.3.0 offline tools PR에서 검증한다. QR의 encoder/dependency/security fixture와 이
> workthrough는 독립적으로 유지한다.

## Overview

Issue #292(P2-03)에 따라 Developer Toolbox에 text·HTTP(S) URL·Wi-Fi payload를 오프라인에서
QR로 변환하는 도구를 추가했다. Tauri에서는 pure-Rust encoder와 고정 PNG/SVG renderer를
우선 사용하고, browser/WebView fallback도 같은 입력·옵션·출력 경계를 적용한다. 결과는
메모리에만 유지하며 사용자가 명시적으로 눌렀을 때만 SVG/PNG를 복사하거나 고정 파일명으로
저장한다.

## Context

- v0.5.0 native-first 계획의 P2-03과 issue #292가 요구한 QR 기능은 외부 QR 서비스나
  runtime download에 의존하지 않아야 했다.
- QR payload에는 URL·Wi-Fi credential-shaped 값이 들어갈 수 있으므로 입력 원문, raw
  credential, 경로, 플랫폼 오류가 로그·오류·DTO·자동 저장으로 새어 나가지 않는 경계를
  먼저 고정했다.
- 기존 계획의 비범위(camera scan, dynamic QR service, 확인 없는 clipboard 저장)는
  유지했다. QR은 encoder와 명시적 export workflow만 제공한다.

## Changes Made

### 1. Bounded native QR core

- File: apps/developer-toolbox/src-tauri/src/core/qr.rs
- File: apps/developer-toolbox/src-tauri/src/commands/qr.rs
- File: apps/developer-toolbox/src-tauri/src/core/mod.rs
- File: apps/developer-toolbox/src-tauri/src/commands/mod.rs
- File: apps/developer-toolbox/src-tauri/src/lib.rs
- qrcode 0.14.1의 default features를 끄고 Byte mode로 UTF-8 bytes를 인코딩한다.
  auto version은 1부터 40까지 smallest-fit를 찾고, 명시 version·L/M/Q/H·quiet zone을
  allowlist와 범위로 검증한다.
- text/URL 전체 payload와 Wi-Fi SSID/password에 각각 UTF-8 바이트 상한을 적용한다.
  URL은 HTTP(S) scheme/authority와 control/whitespace만 확인하고 네트워크를 실행하지
  않는다. Wi-Fi는 WPA/WEP/nopass, hidden, 표준 delimiter escaping을 사용한다.
- deterministic SVG matrix renderer는 payload를 XML에 쓰지 않고 crisp edge path만 만든다.
  png 0.18.1 grayscale/no-filter encoder로 PNG를 만들며 raw image, base64, SVG 모두
  고정 메모리 상한을 넘으면 부분 결과 없이 실패한다.
- Tauri command는 내부 qrcode/png/base64 오류를 한국어 fixed message로 변환하고
  QrResult에는 payload 원문 대신 payload byte count와 image/geometry metadata만 둔다.

핵심 native 경계:

```rust
pub fn generate(request: GenerateQrRequest) -> Result<QrResult, String> {
    generate_inner(request).map_err(|error| error.message().to_string())
}
```

### 2. Browser fallback and native-first API

- File: apps/developer-toolbox/src/tools/qr.ts
- File: apps/developer-toolbox/src/api.ts
- browser 경로는 MIT qrcode-generator 2.0.4를 번들하고 TextEncoder를 명시해 native와
  UTF-8 byte semantics를 맞춘다. qrcode matrix를 직접 조작해 deterministic SVG를 만들고
  canvas PNG를 생성한다.
- invalid preset/type, lone surrogate, malformed URL/Wi-Fi, version/size/quiet/EC,
  capacity, canvas, oversized SVG/PNG 결과를 모두 QrGenerationError의 고정 code/message로
  닫는다. native invoke의 fixed string error도 API 경계에서 알려진 QrGenerationError로
  정규화하며 알 수 없는 오류는 render fixed error로만 노출한다.
- URL fetch, external service, runtime asset download, user path/file picker는 없다.

API 경로는 다음처럼 Tauri가 primary이고 browser는 offline fallback이다.

```typescript
export async function generateQr(request: GenerateQrRequest): Promise<QrResult> {
  if (!isTauri()) return generateBrowserQr(request);
  try {
    return await invoke<QrResult>("generate_qr", { request });
  } catch (error) {
    throw normalizeQrError(error);
  }
}
```

### 3. Explicit QR UI and safe output actions

- File: apps/developer-toolbox/src/tools/QrTool.tsx
- File: apps/developer-toolbox/src/tools/QrTool.test.tsx
- File: apps/developer-toolbox/src/tools/qr.test.ts
- File: apps/developer-toolbox/src/tools/index.tsx
- File: apps/developer-toolbox/src/App.css
- text/URL/Wi-Fi preset, auto/1–40 version, L/M/Q/H, size 64–2,048px, quiet zone 4–16
  modules를 labeled controls로 제공하고 text/URL/wifi 입력을 bounded field로 제한한다.
- 생성은 QR 생성 버튼을 눌렀을 때만 실행한다. 생성 중 controls와 duplicate action을
  잠그고, IME composition 중에는 시작하지 않는다. 입력/option 변경과 unmount는 request
  sequence를 무효화해 늦은 응답·오류가 결과를 덮지 못하게 한다.
- 생성 SVG를 accessible preview와 read-only output으로 표시하고, SVG copy/save와 PNG
  copy/save를 별도 explicit action으로 제공한다. download filename은 devbox-qr.svg/png로
  고정하고 unsafe path를 받지 않는다.
- ClipboardItem/image clipboard가 없거나 실패하면 raw 값·경로·브라우저 오류가 없는
  고정 안내와 PNG 파일 저장 fallback을 사용한다. 공용 ToolTextArea/ToolTextField/
  ToolOutput의 context menu에는 QR용 fixed action error를 적용했다.
- preview alt, labeled preset/options, aria-busy/live status, alert, keyboard/native
  context menu, focusable output을 제공한다. React text rendering과 data URI preview는
  생성된 matrix만 다루고 payload를 DOM markup에 삽입하지 않는다.

### 4. Shared output helper

- File: apps/developer-toolbox/src/tools/common.tsx
- ToolTextField가 password type을 전달할 수 있게 하고 input/output clipboard 실패를
  feature-specific fixed message로 대체할 수 있게 했다.
- explicit binary download helper를 추가했다. base64 decode·Blob·고정 filename·URL
  revoke를 한 경계에서 수행하며 caller가 사용자 action에서만 호출한다.
- 기존 text tools의 기본 오류 동작은 유지하고 QR만 fixedActionError를 전달한다.

### 5. Dependency, metadata, and documentation

- File: apps/developer-toolbox/src-tauri/Cargo.toml
- File: apps/developer-toolbox/package.json
- File: Cargo.lock
- File: pnpm-lock.yaml
- File: THIRD_PARTY_NOTICES.md
- File: docs/dependency-policy.md
- File: apps/developer-toolbox/README.md
- File: docs/roadmap.md
- File: docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md
- qrcode 0.14.1(MIT OR Apache-2.0), png 0.18.1(MIT OR Apache-2.0), qrcode-generator
  2.0.4(MIT)를 exact lock/integrity와 source URL로 고지했다. qrcode optional
  image/svg/pic feature는 사용하지 않는다.
- notices generator를 rebase 후 다시 실행해 HMAC(#424)의 기존 entries와 QR entries를
  함께 유지했다. dependency policy check가 Cargo.lock/pnpm-lock/notices 일치를 확인했다.
- README에는 사용법, bounds, no-network/privacy, native/browser parity, image clipboard
  fallback, 비범위와 의존성 선택을 기록했다. roadmap/native-first plan에는 구현 상태와
  PR/W2 잔여 gate를 상세히 추가했다.

## Code Examples

### Fixed payload and output boundary

```text
const MAX_PAYLOAD_BYTES = 4_096;
const MAX_OUTPUT_SIZE = 2_048;
const MAX_QUIET_ZONE = 16;
// Generate only on an explicit click; never persist or send the payload.
```

### Fixed image clipboard fallback

```typescript
if (!result || typeof ClipboardItem === "undefined" || !navigator.clipboard?.write) {
  setActionError("이 환경에서는 PNG clipboard를 사용할 수 없습니다. PNG 저장을 사용하세요.");
  return;
}
```

## Verification Results

### Rust

```text
source ~/.cargo/env && cargo test -p developer-toolbox --lib -j2
30 passed; 0 failed

cargo check -p developer-toolbox -j2
Finished successfully

cargo clippy -p developer-toolbox --lib --all-targets -j2 -- -D warnings
Finished successfully

cargo fmt --manifest-path apps/developer-toolbox/src-tauri/Cargo.toml --all -- --check
Finished successfully
```

### Frontend

```text
pnpm --dir apps/developer-toolbox exec vitest run src/tools/qr.test.ts src/tools/QrTool.test.tsx
2 files, 8 tests passed

pnpm --dir apps/developer-toolbox test
18 files, 144 tests passed

pnpm --dir apps/developer-toolbox build
Vite production build succeeded
```

### Dependency and diff checks

```text
python3 .github/scripts/check-dependencies.py generate
generated THIRD_PARTY_NOTICES.md

python3 .github/scripts/check-dependencies.py check
dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml

git diff --check
passed
```

## Rebase and Integration Notes

- Feature branch: feat/developer-toolbox/qr-generator
- Initial implementation checkpoint: 115fb42
- Rebased onto latest origin/main 2c20eb5 (HMAC #424) and resolved README,
  api.ts, core/mod.rs, and notices conflicts by retaining both HMAC and QR changes.
- Rebase result before final workthrough amendment: 1d0dae0. Final commit is amended after
  this document and final metadata/diff check.
- HMAC existing direct base64 dependency and notices entries were not duplicated or removed.

## Next Steps and Known Limitations

### Review follow-up (2026-08-27)

- 생성 중 payload field까지 disable하고 변경 callback도 running 상태에서 무시해 실제
  single-flight 경계를 보강했다. 입력 후보는 한 번의 UTF-8 byte 계산으로 text/URL/Wi-Fi
  상한을 먼저 적용한다.
- SVG/PNG copy와 공용 input/output context-menu의 늦은 clipboard 결과가 unmount 또는
  새 action 뒤에 state를 갱신하지 않도록 mounted/action sequence guard를 추가했다.
- PNG raw와 base64 각각의 4 MiB 계약을 일치시키고, URL scheme 대소문자·Unicode
  whitespace 검사를 browser/native에 맞췄다. SVG action label도 구체화했다.
- Review follow-up 뒤 QR focused frontend fixture는 다시 실행해 2 files/8 tests가 통과했다.
  Cargo/build와 Windows W2는 이 review 범위에서 실행하지 않았고 release gate에 남아 있다.

- Windows W2 packaged build/smoke, packaged image clipboard behavior, actual WebView canvas
  output, installer size and release evidence remain root/CI release-gate work.
- Native qrcode-rust and browser qrcode-generator can choose different valid matrices because
  they are separate encoders; shared parity is defined as payload encoding, validation,
  metadata bounds, deterministic behavior per path, and fixed failure contract.
- QR decode/camera scan, dynamic/remote QR generation, automatic clipboard/history/storage,
  and arbitrary save paths are intentionally outside issue #292.
