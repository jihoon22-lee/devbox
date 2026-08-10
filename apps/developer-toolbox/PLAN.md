# developer-toolbox — Developer Toolbox

개발용 소형 도구들을 한 앱에 모은 컬렉션 앱. 기능 하나하나가 작아서 지속 추가하기 좋은 프로젝트. React 사이드바 UI 연습 + Rust 유틸 확장이 목적.
산출물: `DevToolbox.exe`. 모노레포 위치: `devbox/apps/developer-toolbox`.

## 1. 목표
- 자주 쓰는 개발 변환/검사 도구를 오프라인으로 즉시 사용
- 도구 추가가 쉬운 확장 가능한 구조 설계
- JS로 충분한 것과 Rust가 필요한 것의 경계를 경험

## 2. 핵심 기능

### MVP (v1)
| 그룹 | 도구 | 구현 위치 |
|---|---|---|
| JSON | Formatter / Minifier / Validator | TS |
| Encoding | Base64 Encode/Decode, URL Encode/Decode | TS |
| Time | Unix Timestamp ↔ Date | TS |
| Text | Case Converter, Diff | Diff는 Rust(`similar`) |
| Security | Hash (MD5/SHA-256/SHA-512), UUID v4 | **Rust** (`md-5`, `sha2`, `uuid`) |
| Regex | Regex Tester (매치 하이라이트) | **Rust** (`regex`) |
| Auth | JWT Decoder (헤더/페이로드 디코드) | TS (base64url) |

### v2+
- CSV ↔ JSON, Cron 표현 설명, URL Parser, QR 생성
- 도구별 자주 쓰는 설정 저장
- "즐겨찾기" 도구 탭

## 3. 기술 설계

### Rust command (Rust가 실익이 있는 것만)
- `commands/hash.rs` — `hash(data, algorithm)` → hex
- `commands/uuid.rs` — `generate_uuid()` (v4)
- `commands/regex.rs` — `regex_test(pattern, text)` → 매치 목록(위치/그룹)
- `commands/diff.rs` — `diff(a, b)` → 변경 구간 (similar crate)
- 나머지는 프론트 TS 유틸 (`src/lib/`)로 처리 — 오프라인·즉시 응답

### Rust 모듈
- `commands/mod.rs`, `commands/hash.rs`, `commands/uuid.rs`, `commands/regex.rs`, `commands/diff.rs`
- 앱 로컬 `core/` 는 최소 (regex/diff는 크레이트 래핑 수준)
- `models.rs` — `RegexMatch { start, end, group }`, `DiffHunk { old_range, new_range, kind }`
- 앱 공통 UI는 이 앱에서 시작해 `packages/ui`로 추출한다 (사이드바·입력/출력 페어 컴포넌트)

### 확장 구조 (프론트)
```ts
type ToolDef = {
  id: string; group: string; name: string;
  component: React.ComponentType; // 각 도구는 독립 컴포넌트
};
const TOOLS: ToolDef[] = [...]; // 배열에 추가만 하면 좌측 메뉴에 자동 등록
```

## 4. UI 설계
```
Developer Toolbox
▸ JSON      [▶] Formatter  [▶] Minifier  [▶] Validator
▸ Encoding
▸ Time
▸ Hash
▸ UUID
▸ Regex
▸ Diff
▸ JWT
```
- 좌측 그룹 메뉴 (접기/펼치기), 우측 도구 영역
- 공통: 텍스트 입력/출력 2분할, 결과 복사 버튼, 오류 인라인 표시
- CodeMirror를 JSON/Regex/Diff에 적용 (knowledge-base/api-playground과 동일 컴포넌트 → `packages/ui`)

## 5. 구현 단계
1. 스캐폴드 + 사이드바 레이아웃 + ToolDef 레지스트리
2. TS 기반 도구: Base64, URL, Timestamp, Case
3. JSON 도구 (Formatter/Minifier/Validator)
4. Rust 연동 1차: Hash + UUID + command 등록
5. Rust 연동 2차: Regex Tester (하이라이트), Diff
6. JWT Decoder
7. v2 도구 추가 + 설정 저장, 빌드 검증

## 6. 테스트
- Rust: hash/uuid/regex/diff 유닛 테스트 (기대값 픽스처)
- TS: base64/url/timestamp/jwt 순수 함수 vitest
- 각 도구 스모크 테스트

## 7. 확장/연계
- knowledge-base: Markdown 프리뷰 렌더러와 CodeMirror 공용화
- api-playground: JSON 포맷터·Base64를 내부 유틸로 재사용
- 공통 추출 후보: `packages/ui`(입력/출력 페어 컴포넌트·결과 복사 훅·사이드바 레이아웃)

## 8. 완료 정의(Done)
- MVP 도구 14종 전부 동작, 좌측 메뉴에서 탐색 가능
- Rust command 테스트 통과, Windows 빌드 성공
- 도구 추가가 배열 1줄로 끝나는 구조 확인
