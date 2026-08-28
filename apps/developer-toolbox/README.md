# developer-toolbox — Developer Toolbox

개발용 소형 도구를 한 앱에 모은 컬렉션. 도구마다 기능이 작아 지속 확장하기 좋다.
산출물: `DevToolbox.exe` (`apps/developer-toolbox`).

## 도구 목록

| 그룹 | 도구 | 구현 |
|---|---|---|
| JSON | Formatter / Minifier / Validator, JSON ↔ YAML 1.2, JSON → TypeScript | TS (`jsonc-parser`·`yaml`) |
| Encoding | UTF-8 / Hex / Base64 / Base64URL byte codec, 진법 변환, HTML Entity Encode·Decode, URL Component Encode·Decode, QR Generator | TS + Rust |
| Time | Unix Timestamp ↔ Date | TS |
| Text | Case Converter, Lorem Generator, Markdown Table Formatter, Diff | Lorem/table은 deterministic TS, Diff는 Rust(`similar`) |
| Security | Hash(MD5/SHA-256/SHA-512), HMAC-SHA-256/384/512 생성·검증, UUID v4/v7, ULID | Rust(`md-5`·`hmac`·`sha2`·`base64`·`uuid`·`getrandom`) |
| Regex | Regex Tester (매치 하이라이트) | Rust(`regex`) |
| Auth | JWT Decoder / Verify (HS256/HS384/HS512) | TS bounded decoder + RustCrypto HMAC/Web Crypto |

## 주요 특징

- 오프라인 즉시 사용 (외부 서비스 없음)
- 좌측 사이드바에서 도구 선택
- JS로 충분한 것과 Rust가 필요한 것의 경계 분리 — 계산·검증은 Rust 연동
- Lorem Generator는 고정된 로컬 corpus에서 문단·문장·단어를 deterministic하게 생성한다.
  수량은 1–100이고 결과는 UTF-8 65,536바이트 이하로 제한한다. 입력 변경이나 옵션 변경만으로
  결과를 자동 저장·복사하지 않으며, 명시적인 복사·저장 버튼과 결과 context menu에서만
  `lorem-ipsum.txt`로 내보낸다. 수량 paste는 UTF-8 3바이트와 3자리 입력 상한을 함께 적용하고,
  clipboard·download 실패는 고정 오류로만 표시한다.
- Markdown Table Formatter는 붙여 넣은 pipe-delimited 행을 원본 순서대로 정렬·패딩한다.
  선택적인 두 번째 `---`/`:---`/`---:`/`:---:` 행으로 열 정렬을 지정할 수 있으며, 불균일한
  행은 빈 셀로 채우고 구분 행이 없으면 자동 삽입한다. 전체 Markdown 문서 parser/editor나
  HTML renderer가 아니며, 입력 1,000,000바이트·1,000행·100열·셀 4,096 code point·출력
  4,000,000바이트 상한을 fail-closed로 적용한다. `\\|`와 matched backtick code span 안의 pipe를
  cell data로 해석하고 backtick·tag-like text를 데이터로 보존한다.
- 두 Text 도구는 네트워크·외부 converter·random·filesystem read 없이 동일한 bundled TS
  경로에서 동작한다. 입력 변경 시 이전 결과와 action 오류를 지운다. bounded formatter는 다음
  event-loop task에 예약해 먼저 화면을 갱신하고, 시작 전 supersede는 취소하며 시작 뒤에는 엄격한
  상한과 sequence·unmount guard로 오래된 결과를 버린다.
  입력과 출력은 접근 가능한 label/`aria-busy`/live status를 가지며, 일반 cut/copy/paste와
  IME keyboard 동작을 가로채지 않는다. explicit Paste 때만 clipboard를 읽고, paste 결과는
  UTF-8 상한 안에서만 삽입한다.
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
- HMAC는 정확히 `sha256`·`sha384`·`sha512` 알고리즘을 지원한다. key와 message는 각각
  `utf8`, `hex`, 표준 padded `base64`, unpadded RFC 4648 `base64url` 중 하나로 해석하고,
  결과는 lowercase `hex`, padded `base64`, unpadded `base64url` 중 하나로 인코딩한다.
  Hex 입력은 대·소문자를 허용하지만 Base64 계열은 alphabet·padding·pad bit까지 canonical
  표현만 허용한다. 빈 message는 허용하고 key는 비어 있지 않아야 한다.
- HMAC key/message의 decoded 크기는 각각 최대 1,000,000바이트, encoded field는 최대
  2,100,000바이트다. SHA-512 결과를 포함한 tag 출력은 최대 128자이며 초과·잘못된
  algorithm/encoding·malformed input은 원문과 플랫폼 세부사항을 포함하지 않는 하나의 고정
  오류로 거부한다. 검증은 RustCrypto `verify_slice` 또는 Web Crypto `verify`를 사용해
  constant-time primitive 경계를 유지하고, 성공 여부만 반환한다.
- HMAC는 브라우저 미리보기와 Tauri native 경로 모두 외부 network 없이 실행한다. key·message와
  작업 결과는 화면과 한 번의 메모리 작업에만 존재하며 history/localStorage/로그로 저장하거나
  전송하지 않는다. 생성 tag의 복사·결과 파일 저장은 사용자가 출력 메뉴에서 명시적으로
  요청한 경우에만 수행한다. JWT signature verify, secret persistence, pipeline/handoff는
  이 도구의 범위가 아니다.
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

- JWT는 compact `header.payload.signature`를 strict Base64URL·UTF-8·JSON으로 해석해 헤더와
  페이로드를 **검증되지 않음** 상태로 표시한다. `alg=none`, 대소문자 변형, RSA/EC 알고리즘,
  non-canonical padding, 중복 JSON key, 알 수 없는 `crit` header와 잘못된 signature 길이는
  고정 오류로 거부한다. JSON은 64KiB, 32단계, 10,000개 값, 문자열 16KiB, 전체 token
  256KiB의 bounded parser를 사용하며 원문 token·signature를 오류나 로그에 반향하지 않는다.
- Verify는 사용자가 명시적으로 누른 경우에만 실행하고 HS256·HS384·HS512만 허용한다. key는
  raw UTF-8, hex, padded Base64, unpadded Base64URL 중 하나로 명시하며 각각의 decoded key는
  알고리즘 digest 길이 이상(32/48/64바이트)이어야 한다. PEM/JWK/RSA/EC key parsing은 이
  기능의 범위가 아니며 HMAC secret은 UI memory에서만 사용한다. Native는 RustCrypto의
  constant-time `verify_slice`를, browser preview는 Web Crypto HMAC `verify`를 사용하고
  둘 다 key/signature/tag를 반환하지 않는다. Native command도 header/payload JSON bounds,
  duplicate key, critical header, canonical segment를 다시 확인해 direct IPC 호출이 browser
  검사를 우회하지 못하게 한다.
- `exp`·`nbf`·`iat`는 raw NumericDate와 UTC ISO-8601을 함께 표시한다. 검증 시각은 요청
  시작 시 한 번 캡처한 현재 UTC epoch seconds이고 고정 clock skew는 ±60초다. 시간 claim이
  malformed이거나 범위를 벗어나면 signature primitive를 호출하지 않고 invalid 상태로
  표시한다. 유효한 signature와 시간 claim을 모두 통과해야만 `verified`가 된다.
- JWT 입력과 key는 localStorage/history/telemetry/network/자동 clipboard에 절대 기록하지
  않는다. 결과 JSON의 copy/save와 input context-menu Paste는 사용자가 명시적으로 선택한
  경우에만 동작하며, 실행 중 중복 action·IME 입력·늦은 async/native 결과·unmount state
  반영을 막고 accessible label/status/alert를 제공한다.

JSON ↔ YAML의 `jsonc-parser` 3.3.1(MIT)·`yaml` 2.9.0(ISC)과 HTML entity의
`entities` 8.0.0(BSD-2-Clause), JWT verify의 RustCrypto `hmac` 0.13.0(MIT OR
Apache-2.0)·`base64` 0.22.1(MIT OR Apache-2.0)은 앱에 함께 번들된다. 실행 중 다운로드나
network 요청은 없으며 버전·무결성·라이선스는 `Cargo.lock`, `pnpm-lock.yaml`, dependency
policy, `THIRD_PARTY_NOTICES.md`로 검증한다. `cmov`·`ctutils` 등 RustCrypto 전이 의존성도
동일한 lock/notices/dependency gate를 통과해야 한다.

Rust의 HMAC 순수 로직은 `src-tauri/src/core/hmac.rs`, Tauri 경계는
`src-tauri/src/commands/tools.rs`의 `hmac_generate`·`hmac_verify` 명령에 있다. 요청 wire
필드는 `algorithm`, `key`, `keyEncoding`, `message`, `messageEncoding`, `outputEncoding`이며,
검증 요청은 여기에 `expectedTag`를 더한다. Frontend와 native가 같은 strict codec/상한을
공유하고, verify 명령은 `boolean` 외의 tag를 반환하지 않는다.

HMAC가 사용하는 `hmac 0.13.0`은 RustCrypto의 검증된 표준 HMAC primitive(MIT OR
Apache-2.0)이며, `sha2 0.11.0`과 기존에 잠겨 있던 `base64 0.22.1`을 사용한다. 버전·전이
`cmov`/`ctutils`·checksum·license는 루트 `Cargo.lock`과 생성된
`THIRD_PARTY_NOTICES.md`에서 검증한다. 자체 암호 구현, 외부 generator, runtime download는
추가하지 않는다.

- QR Generator는 text, HTTP(S) URL, Wi-Fi preset을 지원한다. URL은 가져오거나 열지 않고
  입력 문자열만 payload로 사용하며, Wi-Fi는 WPA/WEP/nopass·SSID·비밀번호·hidden 필드를
  표준 WIFI 형식으로 escape한다. 텍스트와 URL payload는 UTF-8 4,096바이트, SSID는
  32바이트, 비밀번호는 63바이트까지이며 빈 값·lone surrogate·지원하지 않는 URL/보안
  설정은 고정된 오류로 거부한다.
- QR 버전은 자동(맞는 가장 작은 normal version) 또는 1–40을 선택하고 오류 보정은
  L/M/Q/H를 선택한다. 출력 크기는 64–2,048px, quiet zone은 4–16 modules로 제한하며
  실제 이미지는 module 경계에 맞춰 요청한 최대 크기 이하로 생성된다. 선택한 버전에
  payload가 들어가지 않으면 일부 결과를 만들지 않고 capacity 오류로 중단한다.
- Tauri에서는 feature-gated pure-Rust qrcode 0.14.1을 byte mode로 사용하고 작은
  grayscale PNG encoder(png 0.18.1)와 deterministic SVG renderer로 결과를 만든다.
  browser preview/fallback은 고정 버전 qrcode-generator 2.0.4를 사용하되 동일한
  payload·option bounds, UTF-8 byte semantics, module-aligned dimension 계약을 적용한다.
  두 경로 모두 runtime download, network request, dynamic QR service와 camera scan을
  사용하지 않는다.
- 미리보기와 SVG/PNG 결과는 메모리에만 두며 자동 저장·history·telemetry·clipboard
  write는 없다. 사용자가 명시적으로 SVG/PNG 복사 또는 저장을 눌렀을 때만 action이
  실행되고, SVG/PNG는 고정 파일명으로 저장된다. PNG image clipboard를 지원하지 않는
  WebView/Windows 환경에서는 원문이나 경로를 노출하지 않는 고정 안내와 PNG 파일 저장
  fallback을 제공한다. 생성 오류·clipboard 오류·canvas/encoder 오류도 raw payload,
  credential, path 또는 플랫폼 오류를 반향하지 않는다.
- 생성 요청 중에는 입력과 option을 잠그고 중복 action을 무시한다. 입력 변경·preset 변경과
  unmount는 request sequence를 무효화해 늦은 결과가 화면을 덮어쓰지 않게 하며,
  composition/IME 중에는 생성하지 않는다. preset/option label, live status, alert,
  preview alt text, keyboard/native context menu를 제공해 접근 가능한 explicit workflow를
  유지한다.

QR 기능의 native matrix encoder(qrcode 0.14.1, MIT OR Apache-2.0)와 grayscale PNG
encoder(png 0.18.1, MIT OR Apache-2.0), browser fallback(qrcode-generator 2.0.4, MIT)은
버전과 integrity를 lockfile에 고정한다. qrcode의 optional image/svg/pic feature는 사용하지
않고, 새 license·source·advisory·bundle 크기는 dependency policy와 notice generator의
검사 대상이다.

## Smart Workflows (P3-05, #340–#342)

Smart Workflows는 입력을 한 번 붙여 넣고 안전한 변환 후보를 확인한 뒤, 사용자가 선택한
typed 단계만 실행하는 오프라인 작업 영역이다. 감지·파이프라인·메타데이터 저장은 같은 화면을
공유하지만 각 issue의 fixture와 acceptance는 독립적으로 유지한다.

이번 #340–#343 묶음은 Developer Toolbox 내부의 offline 흐름과 API Playground로 보내는
명시적 handoff 경계를 한 PR에서 제공하되, pipeline 실행과 cross-app 전달은 별도 사용자
동작·저장 계약으로 분리한다.

- **#340 Smart detection** — UTF-8 1,000,000바이트(최대 2,100,000 code unit) 안에서 JSON,
  허용된 HS JWT compact, HTTP(S) URL, canonical Base64/Base64URL, Hex byte 표현을 로컬에서
  판별한다. 결과에는 static kind/tool/transformer ID와 confidence만 들어가며 입력 원문은
  후보·오류·로그에 포함하지 않는다. credential-shaped assignment, bearer/token prefix,
  URL userinfo·credential query, `file://`·Windows/POSIX 경로와 제어문자는 fail-closed한다.
  URL은 절대 열거나 요청하지 않으며 JWT는 Decode 후보일 뿐 Verify가 아니다. 순수 fixture는
  정상·binary·ambiguous·invalid·oversized·credential/path redaction을 각각 고정한다.
- **#341 typed transformer pipeline** — `text`, `json`, `jwt`, `url`, `base64`, `base64url`,
  `hex`, `yaml`, `typescript` 등의 타입을 단계별로 확인하며, 현재 출력 타입을 다음 단계의
  허용 입력 타입과 비교한다. 단계는 최대 8개, 전체 입력 1,000,000바이트, 중간/최종 출력
  4,000,000바이트다. incompatible/unknown 단계는 실행 전에 고정 오류로 중단하고, 단계 실행은
  파이프라인 실행 버튼을 눌렀을 때만 한다. shell command, network, `api-request/v1`,
  `toolbox-text/v1` receiver는 이 issue에 없다.
- **#342 recent/favorite 저장** — native Tauri 실행은 `app_local_data_dir()` 아래
  `smart-workflows.json`을 version 1 metadata로 원자 교체한다. 브라우저 preview만 같은
  allow-listed schema를 `localStorage`에 저장한다. 저장되는 것은 tool ID, transformer ID,
  pipeline 입력 타입, pipeline ID와 timestamp뿐이며 input/output/clipboard/credential/path와
  사용자 지정 이름은 스키마에 존재하지 않는다. recent 20개, favorite 50개, pipeline 20개,
  단계 8개, serialized metadata 64KiB를 넘으면 안전하게 버린다. malformed/unknown entry는
  화면에 반향하지 않고 fail-closed한다. 재시작 시 metadata만 복원하며 draft text는 복원하지
  않는다.
- 입력 context menu의 Paste는 기존 공용 `ToolTextArea`를 통해 사용자가 누른 순간에만 읽고,
  UTF-8 byte 상한을 넘는 부분은 삽입하지 않는다. 결과 copy/save도 `ToolOutput`의 명시적
  action에서만 가능하며 smart workflow는 clipboard history, raw archive, API handoff를
  만들지 않는다. 오류는 fixed message로만 표시한다.

### Issue별 acceptance/fixture 추적

| 이슈 | 독립적으로 확인하는 계약 | 집중 fixture |
|---|---|---|
| #340 | 구조 후보·추천 transformer, ambiguous/invalid, credential/path 비반향 | `smartDetection.test.ts` |
| #341 | output→input 타입 연결, mismatch 선차단, bounded local run | `transformPipeline.test.ts` |
| #342 | metadata schema redaction, bounded ordering, restart round-trip, atomic native file | `workflowStore.test.ts`, Rust `core::workflows` tests |

packaged Windows W3 smoke는 각 후보 선택, mismatch 표시, pipeline 실행, 재시작 후
metadata-only 복원, handoff preview/edit/cancel/apply와 no-auto-send를 확인해야 한다.

## API Playground handoff (`api-request/v1`, #343)

각 결과 화면의 `API Playground로 보내기`는 사용자가 보고 있는 현재 output만 대상으로 하는
명시적 수동 handoff다. 먼저 `POST /`와 `text/plain; charset=utf-8` draft를 미리 보여 주고,
body를 편집한 뒤 `API Playground로 전달`을 눌러야 공용 AppLink protocol v2의 opaque one-time
handoff store에 bounded envelope가 만들어진다. API Playground는 이를 claim해 preview한 뒤
사용자가 적용할 때만 request editor에 넣으며 요청을 자동으로 보내지 않는다.

output은 256,000 chars·1,024,000 UTF-8 bytes와 NUL 입력 상한을 넘을 수 없고, shared handoff
validator가 raw credential과 unsafe path field를 fail-closed로 거부한다. producer는 input/history,
secret, raw credential, file path를 저장하거나 argv·로그·clipboard로 보내지 않는다. 대상 앱이
설치되지 않았거나 실행에 실패해도 clipboard fallback은 없다. 실행 실패 시 아직 pending인 정확한
envelope를 폐기하고, 브라우저 preview에서는 native handoff를 사용할 수 없다는 고정 오류만 표시한다.
in-flight 전달 중 output이 바뀌어도 두 번째 publish를 시작하지 않으며 완료 상태에 opaque ID를
노출하지 않는다.

## 개발

- 순수 로직: `src-tauri/src/core/hmac.rs`·`src-tauri/src/core/workflows.rs`·
  `src-tauri/src/core/handoff.rs` 및 `src-tauri/src/commands/tools.rs` → `cargo test`
- Smart Workflows의 감지·typed pipeline·metadata 테스트는 `src/workflows/*.test.ts(x)`에
  issue별 fixture로 분리한다. Rust metadata 파일은 `app_local_data_dir()` 내부의
  `smart-workflows.json`만 원자 교체한다.
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`
