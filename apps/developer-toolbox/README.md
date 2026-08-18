# developer-toolbox — Developer Toolbox

개발용 소형 도구를 한 앱에 모은 컬렉션. 도구마다 기능이 작아 지속 확장하기 좋다.
산출물: `DevToolbox.exe` (`apps/developer-toolbox`).

## 도구 목록

| 그룹 | 도구 | 구현 |
|---|---|---|
| JSON | Formatter / Minifier / Validator | TS |
| Encoding | Base64 / URL Encode·Decode | TS |
| Time | Unix Timestamp ↔ Date | TS |
| Text | Case Converter, Diff | Diff는 Rust(`similar`) |
| Security | Hash(MD5/SHA-256/SHA-512), UUID v4 | Rust(`md-5`·`sha2`·`uuid`) |
| Regex | Regex Tester (매치 하이라이트) | Rust(`regex`) |
| Auth | JWT Decoder (헤더/페이로드) | TS(base64url) |

## 주요 특징

- 오프라인 즉시 사용 (외부 서비스 없음)
- 좌측 사이드바에서 도구 선택
- JS로 충분한 것과 Rust가 필요한 것의 경계 분리 — 계산·검증은 Rust 연동

## 개발

- 순수 로직: `src-tauri/src/commands/tools.rs` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

