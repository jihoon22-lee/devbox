# developer-toolbox — Developer Toolbox

개발용 소형 도구를 한 앱에 모은 컬렉션. 도구마다 기능이 작아 지속 확장하기 좋다.
산출물: `DevToolbox.exe` (`apps/developer-toolbox`).

## 도구 목록

| 그룹 | 도구 | 구현 |
|---|---|---|
| JSON | Formatter / Minifier / Validator, JSON ↔ YAML 1.2, JSON → TypeScript | TS (`jsonc-parser`·`yaml`) |
| Encoding | UTF-8 / Hex / Base64 / Base64URL byte codec, 진법 변환, HTML Entity Encode·Decode, URL Component Encode·Decode | TS |
| Time | Unix Timestamp ↔ Date | TS |
| Text | Case Converter, Diff | Diff는 Rust(`similar`) |
| Security | Hash(MD5/SHA-256/SHA-512), UUID v4/v7, ULID | Rust(`md-5`·`sha2`·`uuid`·`getrandom`) |
| Regex | Regex Tester (매치 하이라이트) | Rust(`regex`) |
| Auth | JWT Decoder (헤더/페이로드) | TS(base64url) |

## 주요 특징

- 오프라인 즉시 사용 (외부 서비스 없음)
- 좌측 사이드바에서 도구 선택
- JS로 충분한 것과 Rust가 필요한 것의 경계 분리 — 계산·검증은 Rust 연동
- UUID / ULID 생성기는 UUID v4·v7과 canonical Crockford Base32 ULID를 한 번의 batch
  요청으로 최대 100개까지 생성한다. UUID는 표준 hyphen/compact와 대·소문자를 선택할 수
  있고, ULID는 canonical 대문자·hyphenless 형식을 기본으로 하며 표시용 그룹 hyphen도
  지원한다. UUID v7과 ULID는 시계가 같은 밀리초에 머물거나 뒤로 이동해도 단일 batch
  내부에서 생성 순서대로 엄격히 증가한다. UUID v4는 순서를 보장하지 않으며, 별도 호출·
  프로세스·컴퓨터 사이의 전역 순서도 보장하지 않는다. 플랫폼의 보안 난수를 사용할 수
  없을 때는 고정된 오류로 중단하고 취약한 난수로 대체하지 않는다. 생성 값은 자동 저장·
  전송하지 않는다.
- JSON ↔ YAML은 strict JSON과 YAML 1.2 문서 하나를 양방향 변환하고, 구문 오류의 1-based
  행·열과 안전한 오류 code를 표시한다. 입력은 1,000,000바이트, 출력은 4,000,000바이트,
  YAML alias 확장은 50개로 제한하며 merge key는 자동 확장하지 않는다. JSON에서 안전하게
  표현할 수 없는 정수·비유한 숫자와 의미가 손실되는 YAML custom tag도 명시적으로 거부한다.
- YAML → JSON은 주석을 제거하고 anchor/alias를 값으로 확장한다. 변환 화면에 이 손실과
  anchor 이름·공유 관계가 보존되지 않는다는 안내를 항상 표시하며, 결과를 복사하거나
  `.json`/`.yaml` 파일로 저장할 수 있다.
- JSON → TypeScript는 사용자가 지정한 ASCII TypeScript root identifier로 `export interface`
  또는 `export type`을 만든다. object key는 code-point 순서로 정렬하고 배열 속 object 표본은
  한 번에 구조 병합해 누락 속성을 optional로 추론한다. primitive·null union과 중첩 object·
  array를 보존하며, 빈 배열은 `Array<unknown>`, 빈 object는 `Record<string, never>`로 명시한다.
- JSON → TypeScript 입력은 strict JSON 1,000,000바이트, 중첩 64단계, 값 100,000개, 출력
  4,000,000바이트로 제한한다. 원본 값은 생성 코드나 오류에 포함하지 않고 자동 저장·전송하지
  않으며 사용자가 명시적으로 복사하거나 `.ts` 파일로 저장할 때만 외부 action을 수행한다.
- byte codec은 입력·출력 표현을 UTF-8 text, Hex raw bytes, Base64, unpadded Base64URL에서
  각각 선택한다. 내부에서는 최대 1,000,000 raw byte를 `Uint8Array`로 보존하고 입력 표현은
  최대 2,100,000자로 제한한다. Hex와 Base64 계열의 ASCII 공백은 paste 편의를 위해 무시한다.
- invalid Hex/Base64 character·padding·pad bit는 원문 문자 위치를, overlong·truncated·surrogate·
  범위 초과 UTF-8은 raw byte 위치를 표시한다. 잘못된 UTF-8을 대체 문자로 바꾸지 않으며
  Base64URL은 RFC 4648의 `-`/`_` alphabet으로 구분해 padding 없이 출력한다.
- Base64는 암호화나 secret 보호 수단이 아니다. codec 입력·결과는 자동 저장·전송하지 않고,
  사용자가 명시적으로 누른 복사·text 파일 저장 action만 수행한다.
- 진법 변환은 2·8·10·16진수를 동시에 보여 준다. 자동 입력은 부호 뒤의 `0b`·`0o`·`0x`를
  감지하고 prefix가 없으면 10진수로 읽으며, 명시적 입력 진법에서는 prefix 일치 여부를
  검증한다. invalid digit·prefix와 256bit 범위 초과가 시작되는 원문 문자 위치를 표시한다.
- 진법 결과는 sign-before-prefix signed magnitude이고 two's complement 해석이나 digit
  separator는 지원하지 않는다. 입력 표현은 512자, 절댓값은 최대 256bit로 제한하며 `BigInt`는
  이 범위 안의 정확한 계산에만 사용한다.
- HTML entity와 URL component 도구는 브라우저 parser·외부 converter·network 없이 현재 WebView의
  순수 text pipeline에서 즉시 실행한다. HTML encode는 `&`, `<`, `>`, `"`, `'`만 각각
  `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&#39;`로 canonical하게 바꾸며 다른 Unicode 문자는 보존한다.
  decode는 BSD-2-Clause `entities@8.0.0` codec의 전체 표준 HTML named entity와 직접 검증한
  decimal/hex numeric entity를 semicolon이 있는 strict mode로만 해석한다. `&`가
  entity처럼 시작하지만 unknown·unterminated·invalid code point·surrogate이면 조용히 복구하지
  않고 고정 오류로 중단하며, entity를 시작할 수 없는 일반 literal `&`만 보존한다. HTML parser나
  sanitizer가 아니므로 tag/attribute 해석, entity 없는 HTML 문서 정규화는 제공하지 않는다.
- URL component encode/decode는 `encodeURIComponent`/`decodeURIComponent`의 component semantics를
  사용해 query/path 전체 URL을 조립하지 않는다. `%` escape는 정확히 두 hexadecimal digit이어야
  하고 percent-decoded bytes도 유효한 UTF-8이어야 한다. malformed escape, invalid UTF-8, lone
  JS surrogate는 고정 오류로 거부해 replacement character나 부분 결과를 만들지 않는다.
- 두 codec 모두 UTF-8 input 1,000,000바이트, output 4,000,000바이트, 최대 expansion ratio 16을
  적용하고, HTML decode는 entity token 32자·numeric digit 7자·entity 100,000개를 넘으면
  fail-closed한다. encode는 output을 만들기 전 예상 크기를 계산해 oversized expansion을
  차단하고, decode는 output을 누적할 때도 같은 상한을 재확인한다. 문자열 원문·secret·credential·
  path와 플랫폼/URI parser 오류는 오류 메시지·로그에 반향하지 않으며 오류 시 output은 빈 값이다.
- 입력 변경 중 이전 output을 지우고 현재 변환만 표시한다. sequence guard가 늦게 끝난 async
  결과를 버리고 `aria-busy`/live running status로 상태를 알린다. 기존 공용 input context menu의
  명시적 Paste·전체 선택·비우기, native cut/copy/paste와 IME keyboard 동작, output의 명시적
  copy/select/save 및 접근 가능한 Input/Output label을 그대로 사용한다. 자동 저장·history·
  clipboard read/network 요청은 없고 clipboard와 파일 저장은 사용자가 누른 action에서만 수행한다.
  effect cleanup은 unmount 뒤 늦게 도착한 transform 결과의 state 반영도 차단한다.
- 입력 우클릭 메뉴에서 명시적 Paste·전체 선택·비우기, 출력 메뉴에서 복사·전체 선택·텍스트
  파일 저장 지원. Clipboard read는 Paste를 누른 순간에만 수행하며 저장·로그하지 않음

JSON ↔ YAML의 `jsonc-parser` 3.3.1(MIT)·`yaml` 2.9.0(ISC)과 HTML entity의
`entities` 8.0.0(BSD-2-Clause)은 앱에 함께 번들된다. 실행 중
다운로드나 network 요청은 없으며 버전·무결성·라이선스는 `pnpm-lock.yaml`, dependency policy,
`THIRD_PARTY_NOTICES.md`로 검증한다.

## 개발

- 순수 로직: `src-tauri/src/commands/tools.rs` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`
