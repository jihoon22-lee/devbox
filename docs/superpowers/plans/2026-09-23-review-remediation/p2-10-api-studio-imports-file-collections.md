# P2-10 API Studio 보강 1: 가져오기·파일 컬렉션(PR 2개) — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4와 `CONVENTIONS.md`의 "메서드 추가 절차"(P1-11)를 읽는다.

**Goal:** API Studio에 curl 붙여넣기, Postman v2.1, Insomnia v4, Bruno(`.bru`), HAR 가져오기를 더하고(PR A), 컬렉션을 요청마다 파일 하나인 폴더로 내보내고 다시 가져오는 "파일 컬렉션"을 더한다(PR B). 프로젝트 저장소에 API 요청을 함께 커밋할 수 있게 하는 것이 목적이다(D21, D25 4순위, `review.md` §8 표 5번).

**Architecture:**
- 변환은 모두 프런트의 순수 함수다: `packages/api-studio-features/src/requests/lib/importers/{curl,postman,insomnia,har,bruno,index}.ts`. 각 importer는 `ImportBundle { requests: ImportedRequest[], environments: ImportedEnvironment[], warnings: string[] }`을 만들고, `toImportPreview(bundle)`가 기존 JSON 가져오기와 같은 경로로 바꾼다: 요청은 `sanitizeRequestForPersistence`(평문 비밀은 `[REDACTED]` + `requiresSecretReview`), 환경은 비밀로 보이는 값(`isSensitiveName`·`looksLikeSecret`)을 비운 `secret: true` 변수. 병합은 기존 `mergeImportedCollections`·`mergeImportedEnvironments`.
- native는 파일 읽기·쓰기만 한다(`crates/http-client-engine/src/commands/transfer.rs` 옆 새 파일): 여러 파일 읽기(형식별 필터, 크기 한도), 폴더 고르기(10분짜리 opaque grant), 폴더 읽기·쓰기(링크 거부, 깊이·개수 한도). 기존 `read_json_file`의 Devbox 스키마 검사는 그대로 둔다(외부 형식은 새 명령으로).
- 파일 컬렉션 형식: 폴더 루트의 `collection.devbox.json`(`{"schema":"devbox.api-studio.file-collection","schemaVersion":1,"name":…}`)과 요청마다 `<폴더 경로>/<이름>.request.json`(`{"schema":"devbox.api-studio.request","schemaVersion":1,"name":…,"request":<PersistedHistoryRequest>}`). 환경은 넣지 않는다(비밀 경계 유지). 내보내기는 빈 폴더나 같은 표시 파일이 있는 폴더에만 쓰고, 남은 옛 요청 파일은 지우지 않고 목록으로 알린다.
- 가져오기 결과는 적용 전 미리보기(요청 체크 목록·경고)에서 고른다. 적용 뒤 8초 되돌리기(P1-18 `useUndo`: 이전 문서로 조건부 되돌림, P1-17 `DocumentStore` revision 사용).

**Tech Stack:** TypeScript·React 19, Vitest, Rust(Tauri dialog)

**Spec:** `review.md` §8 신규 기능 표 5번 · `00-roadmap.md` D21·D25

## Global Constraints

- `00-roadmap.md` §3 전부 적용. 새 의존성 없음(파서는 직접 작성).
- 한도: 파일 하나 16MiB(HAR), 읽기 합계 32MiB, 파일 500개(폴더는 1,000개·깊이 8), 가져오는 요청 합계는 기존 `MAX_EXPORTED_COLLECTIONS`, 환경 변수는 `MAX_EXPORTED_VARIABLES`.
- 변수 문법은 Devbox의 `{{name}}`으로 맞춘다(Insomnia `{{ _.name }}`·Bruno `{{name}}`·Postman `{{name}}`).
- 스크립트(Postman event, Bruno script·tests, Insomnia 플러그인 태그)는 실행하지 않고 가져오지도 않는다. 개수를 경고로 알린다.
- 파일 본문(`-d @file`, formdata 파일 파트)은 경로를 가져오지 않는다(파일 이름만 남기고 다시 고르게 한다).
- 요청 방법 목록(`METHODS`)에 `HEAD`, `OPTIONS`를 더한다(가져온 요청이 목록 밖 값을 갖지 않게).

## Review Focus

1. 평문 토큰이 든 Postman·curl 요청은 저장 전에 `[REDACTED]`가 되고 "비밀 검토 필요"로 표시된다. (PR A Task A1·A2 테스트)
2. json·form 본문으로 바뀐 요청에서 원래 `Content-Type` 헤더는 빠진다(native가 넣으므로 중복 전송 방지). (A1 테스트)
3. Windows "Copy as cURL (cmd)" 형식(`^"`, `^` 줄바꿈)도 bash 형식과 같은 결과가 나온다. (A1 테스트)
4. 깨진 파일·모르는 형식은 "가져올 수 없는 형식" 한 줄로 끝나고 기존 컬렉션은 그대로다. (A6 테스트)
5. 파일 컬렉션 내보내기는 다른 파일이 있는 폴더(표시 파일 없음)에 쓰지 않는다. (PR B Task B2 테스트)

---

## PR A — 가져오기

- 묶음: **B12** — 브랜치 `feat/devbox-api-studio/imports-runner-auth`, PR 제목 `feat(devbox-api-studio): imports, file collections, runner, OAuth 2.0, TLS and code generation`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `feat(devbox-api-studio): import curl, Postman, Insomnia, Bruno and HAR`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

### Task A1: 가져오기 모델과 curl

**Files:** Create `packages/api-studio-features/src/requests/lib/importers/{index.ts,curl.ts,curl.test.ts,preview.test.ts}`; Modify `lib/transfer.ts`(`isSensitiveName`, `looksLikeSecret` export), `App.tsx` 또는 P1-15에서 나눈 요청 편집 부품의 `METHODS`

**Interfaces (Produces):**
- `index.ts`: `interface ImportedRequest { name: string; folder: string; request: RequestTemplate }`, `interface ImportedEnvironment { name: string; variables: { key: string; value: string }[] }`, `interface ImportBundle { requests: ImportedRequest[]; environments: ImportedEnvironment[]; warnings: string[] }`, `type ImportFormat = "curl" | "devbox" | "postman" | "insomnia" | "har" | "bruno"`, `detectFormat(fileName: string, text: string): ImportFormat | null`, `parseImport(format: ImportFormat, files: { relativePath: string; text: string }[]): ImportBundle`(실패는 `ImportError`), `class ImportError extends Error`, `toImportPreview(bundle: ImportBundle, makeId: () => string): { collections: CollectionStore; environments: EnvironmentExportDocument; warnings: string[] }`, `emptyRequest(): RequestTemplate`
- `curl.ts`: `parseCurl(command: string): ImportBundle`(요청 1개), `tokenizeShell(command: string): string[]`

- [ ] **Step 1: 실패하는 테스트** — `curl.test.ts`

```ts
import { describe, expect, it } from "vitest";
import { parseCurl, tokenizeShell } from "./curl";

describe("curl import", () => {
  it("tokenizes quotes, escapes and line continuations", () => {
    expect(tokenizeShell(`curl 'a b' "c \\"d\\"" $'e\\nf' \\\n  -H x`)).toEqual(["curl", "a b", 'c "d"', "e\nf", "-H", "x"]);
  });

  it("maps a JSON POST and drops the redundant content type", () => {
    const { requests, warnings } = parseCurl(`curl -X POST 'https://api.example.com/users?x=1' -H 'Content-Type: application/json' -H 'Accept: application/json' --data-raw '{"name":"kim"}' --compressed`);
    const request = requests[0].request;
    expect(request.method).toBe("POST");
    expect(request.url).toBe("https://api.example.com/users?x=1");
    expect(request.body_kind).toBe("json");
    expect(request.body).toBe('{"name":"kim"}');
    expect(request.headers.map((h) => h.key)).toEqual(["Accept"]);
    expect(warnings).toEqual([]);
  });

  it("infers POST from data, form-encodes url data and handles -G", () => {
    expect(parseCurl(`curl https://x.test -d a=1 -d b=2`).requests[0].request).toMatchObject({ method: "POST", body_kind: "form", body: "a=1\nb=2" });
    expect(parseCurl(`curl -G https://x.test/s --data-urlencode 'q=a b'`).requests[0].request).toMatchObject({ method: "GET", url: "https://x.test/s?q=a%20b", body_kind: "none" });
  });

  it("maps basic and bearer auth, cookies and user agent", () => {
    const bearer = parseCurl(`curl https://x.test -H 'Authorization: Bearer {{token}}'`).requests[0].request;
    expect(bearer.auth).toMatchObject({ kind: "bearer", token: "{{token}}" });
    expect(bearer.headers).toEqual([]);
    const basic = parseCurl(`curl -u kim:secret https://x.test -b 'a=1; b=2' -A 'devbox'`).requests[0].request;
    expect(basic.auth).toMatchObject({ kind: "basic", username: "kim", password: "secret" });
    expect(basic.cookies).toEqual([{ name: "a", value: "1", enabled: true }, { name: "b", value: "2", enabled: true }]);
    expect(basic.headers).toEqual([{ key: "User-Agent", value: "devbox", enabled: true }]);
  });

  it("reads Windows cmd copies the same way", () => {
    const cmd = parseCurl(`curl ^"https://api.example.com/users^" ^\n  -H ^"Accept: application/json^" ^\n  --data-raw ^"^{^\\^"a^\\^":1^}^"`).requests[0].request;
    const bash = parseCurl(`curl 'https://api.example.com/users' -H 'Accept: application/json' --data-raw '{"a":1}'`).requests[0].request;
    expect(cmd).toEqual(bash);
  });

  it("warns about files, insecure mode and unknown options", () => {
    const { requests, warnings } = parseCurl(`curl -k --data-binary @payload.json -F 'doc=@/home/me/a.pdf' --retry 3 https://x.test`);
    expect(requests[0].request.body).toBe("");
    expect(warnings).toEqual(expect.arrayContaining([
      "파일 본문(@payload.json)은 가져오지 않았습니다.",
      "파일 파트 doc는 파일을 다시 선택해야 합니다.",
      "인증서 검증 끄기(-k)는 가져오지 않았습니다.",
      "지원하지 않는 옵션 --retry를 건너뛰었습니다.",
    ]));
  });

  it("rejects text that is not a curl command", () => {
    expect(() => parseCurl("wget https://x.test")).toThrow("curl 명령이 아닙니다");
  });
});
```

  `preview.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { toImportPreview, emptyRequest } from "./index";

describe("import preview", () => {
  it("redacts literal secrets and blanks secret-looking environment values", () => {
    let n = 0;
    const preview = toImportPreview({
      requests: [{ name: "Me", folder: "Users", request: { ...emptyRequest(), url: "https://x.test/me", auth: { kind: "bearer", username: "", password: "", token: "ghp_abcdefghijklmnopqrstuvwxyz0123", api_key: "", api_value: "" } } }],
      environments: [{ name: "dev", variables: [{ key: "baseUrl", value: "https://x.test" }, { key: "apiToken", value: "abc" }] }],
      warnings: [],
    }, () => `id-${++n}`);
    const entry = preview.collections.collections[0];
    expect(entry.request.auth?.token).toBe("[REDACTED]");
    expect(entry.requiresSecretReview).toBe(true);
    expect(preview.environments.environments[0].variables).toEqual([
      { key: "baseUrl", value: "https://x.test", secret: false },
      { key: "apiToken", reference: "${apiToken}", secret: true },
    ]);
  });
});
```

  (`environments[0].variables`의 비밀 모양은 기존 `serializeEnvironmentExport`가 만드는 `ExportEnvironmentVariable`과 같아야 한다 — 실제 필드 이름이 다르면 그 모양에 맞춘다.)
- [ ] **Step 2: 실패 확인** — Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/requests/lib/importers` → FAIL
- [ ] **Step 3: 구현**
  - `tokenizeShell`: 먼저 줄 이음(`\`+줄바꿈, `^`+줄바꿈, `` ` ``+줄바꿈)을 공백으로 바꾼다. 입력에 `^"`가 있으면 cmd 형식으로 보고 `^X`를 `X`로 푼다(`^"`→`"`, `^^`→`^`, `^\`→`\`, `^{`→`{`, `^}`→`}`, `^&`→`&`, `^|`→`|`, `^<`→`<`, `^>`→`>`, `^%`→`%`). 그다음 bash 규칙: 공백 구분, `'…'`는 그대로, `"…"`는 `\"`·`\\`·`\$`·`` \` ``만 풀기, `$'…'`는 `\n`·`\t`·`\\`·`\'`·`\"` 풀기, 따옴표 밖 `\X`는 `X`.
  - `parseCurl`: 첫 토큰이 `curl`/`curl.exe`가 아니면 `ImportError("curl 명령이 아닙니다")`. 옵션: `-X/--request`, `-H/--header`, `-d/--data/--data-raw/--data-ascii/--data-binary`(값이 `@`로 시작하면 경고·본문 없음), `--data-urlencode`(`name=value`의 value를 `encodeURIComponent`), `--json`(본문 + `json` 종류 + Accept 헤더), `-F/--form`(multipart 텍스트 파트, `name=@path`는 파일 파트 `file_name`만·경고), `-u/--user`(basic), `-b/--cookie`(`a=1; b=2`), `-A/--user-agent`, `-e/--referer`, `-G/--get`, `-I/--head`, `--url`, `--max-time`(초 → `timeout_ms`), `-o/--output`(값 무시), 무시: `-L -s -S -v -i --compressed`, `-k/--insecure`는 경고. `-XPOST`처럼 붙은 값과 `-sSL`처럼 묶인 플래그를 푼다. 값을 받는 옵션 중 쓰지 않는 것(`--retry`, `--retry-delay`, `--retry-max-time`, `--connect-timeout`, `-w/--write-out`, `-x/--proxy`, `--resolve`, `--cacert`, `-E/--cert`, `--key`, `-c/--cookie-jar`, `-D/--dump-header`, `-T/--upload-file`, `--limit-rate`, `--interface`)는 값까지 건너뛰고 "지원하지 않는 옵션 X를 건너뛰었습니다." 경고, 이 목록에도 없는 옵션은 그 토큰만 건너뛰고 같은 경고. 따옴표 없는 첫 비옵션 토큰이 URL(스킴이 없으면 `http://`를 붙임). 방법 기본값은 본문이 있으면 POST, 없으면 GET. `-G`면 데이터를 URL 쿼리로 옮긴다. 본문 종류: `--json` 또는 Content-Type이 json이면 `json`, `application/x-www-form-urlencoded`이거나 `-d`의 기본이면 `form`(`&`를 줄바꿈으로), `-F`가 있으면 `multipart`, 그 밖 `raw`. json·form·multipart면 원래 `Content-Type` 헤더를 뺀다. `Authorization: Bearer X`는 bearer 인증으로 옮긴다. 이름은 `<METHOD> <경로>`.
  - `toImportPreview`: 요청마다 `sanitizeRequestForPersistence` → `CollectionEntry { id: makeId(), name, folder, saved_at: Date.now(), request, requiresSecretReview }`; 환경 변수는 `isSensitiveName(key) || looksLikeSecret(value)`면 비밀(값 없음), 아니면 값 그대로. 폴더·이름은 기존 export 한도(`MAX_TRANSFER_NAME_CHARS`)로 자르고 비면 `untitled`.
  - `METHODS`에 `HEAD`, `OPTIONS`.
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS. `git add -A && git commit -m "feat(devbox-api-studio): import curl commands"`

---

### Task A2: Postman v2.1

**Files:** Create `lib/importers/{postman.ts,postman.test.ts}`, `packages/api-studio-features/src/requests/lib/importers/fixtures/postman-v21.json`

- [ ] **Step 1: fixture** — 폴더 두 단계(`Users` → `Admin`), 요청 4개: (1) GET `{{baseUrl}}/users` 헤더 2개(하나 `disabled: true`), (2) POST raw json(`options.raw.language: "json"`) + bearer `{{token}}`, (3) urlencoded 본문(항목 하나 disabled), (4) graphql 본문. 컬렉션 `auth`는 apikey(header `X-Key`, 값 `{{key}}`), 요청 (1)은 auth 없음(상속), 요청 (4)는 `auth: {type: "oauth2"}`. 컬렉션 `variable` 2개, 요청 (2)에 `event` 1개.
- [ ] **Step 2: 실패하는 테스트**

```ts
import { describe, expect, it } from "vitest";
import fixture from "./fixtures/postman-v21.json";
import { parsePostman } from "./postman";

describe("Postman v2.1 import", () => {
  const bundle = parsePostman(JSON.stringify(fixture));
  it("keeps the folder tree and variables", () => {
    expect(bundle.requests.map((r) => `${r.folder}|${r.name}`)).toEqual([
      "Users|List users", "Users|Create user", "Users/Admin|Login form", "Users/Admin|Query",
    ]);
    expect(bundle.environments[0].name).toBe("Postman: Devbox demo");
    expect(bundle.environments[0].variables.map((v) => v.key)).toEqual(["baseUrl", "key"]);
  });
  it("maps headers, bodies and inherited auth", () => {
    const [list, create, form, query] = bundle.requests.map((r) => r.request);
    expect(list.url).toBe("{{baseUrl}}/users");
    expect(list.headers.find((h) => h.key === "X-Trace")?.enabled).toBe(false);
    expect(list.auth).toMatchObject({ kind: "apikey", api_key: "X-Key", api_value: "{{key}}" });
    expect(create).toMatchObject({ method: "POST", body_kind: "json" });
    expect(create.auth).toMatchObject({ kind: "bearer", token: "{{token}}" });
    expect(form).toMatchObject({ body_kind: "form", body: "user=kim" });
    expect(query.body_kind).toBe("graphql");
    expect(query.graphql?.query).toContain("query");
  });
  it("warns about scripts and unsupported auth", () => {
    expect(bundle.warnings).toEqual(expect.arrayContaining([
      "스크립트 1개는 가져오지 않았습니다.",
      "Query: 지원하지 않는 인증 방식(oauth2)은 가져오지 않았습니다.",
    ]));
  });
  it("rejects other schemas", () => {
    expect(() => parsePostman(JSON.stringify({ info: { schema: "https://schema.getpostman.com/json/collection/v1.0.0/" } }))).toThrow();
  });
});
```

- [ ] **Step 3: 실패 확인** — Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/requests/lib/importers/postman.test.ts` → FAIL
- [ ] **Step 4: 구현** — `info.schema`가 `collection/v2.1` 또는 `collection/v2.0`을 포함해야 한다. `item`을 재귀로 돌며 폴더 이름을 `/`로 잇는다. url은 문자열이거나 `{raw}`. 헤더 `{key, value, disabled}`. body `mode`: raw(`options.raw.language === "json"` 또는 Content-Type json → json, 아니면 raw), urlencoded → form(활성 항목만 `key=value` 줄), formdata → multipart(파일 파트는 경고), graphql → `{query, variables: 문자열, operation_name: ""}`, file → 본문 없음·경고. auth: bearer·basic·apikey(`in: "query"`면 쿼리 파라미터로 옮기고 경고), `noauth`는 상속 끊기, 없으면 상위 폴더·컬렉션 auth 상속, 그 밖 경고. `event` 개수는 전체 합쳐 경고 한 줄. Content-Type 제거 규칙은 curl과 같다(공용 함수 `normalizeBodyHeaders`를 `index.ts`에 둔다).
- [ ] **Step 5: 확인·커밋** — Run: Step 3 명령 → PASS. `git add -A && git commit -m "feat(devbox-api-studio): import Postman collections"`

---

### Task A3: Insomnia v4

**Files:** Create `lib/importers/{insomnia.ts,insomnia.test.ts}`, `fixtures/insomnia-v4.json`

- [ ] **Step 1: fixture** — `_type: "export"`, `__export_format: 4`, resources: workspace 1, request_group 1(`Auth`), request 3(`GET {{ _.baseUrl }}/me` + bearer `{{ _.token }}`; `POST` form(`application/x-www-form-urlencoded`, params 2개 중 하나 disabled); `POST` json 본문과 `parameters` 쿼리 1개), environment 2개(base: `{"baseUrl":"https://x.test","token":"t"}`, sub `staging`: `{"baseUrl":"https://staging.x.test"}`), 요청 하나의 URL에 `{% response 'body', 'req_1', '$.id' %}` 태그.
- [ ] **Step 2: 실패하는 테스트**

```ts
import { describe, expect, it } from "vitest";
import fixture from "./fixtures/insomnia-v4.json";
import { parseInsomnia } from "./insomnia";

describe("Insomnia v4 import", () => {
  const bundle = parseInsomnia(JSON.stringify(fixture));
  it("normalizes template variables and folders", () => {
    const me = bundle.requests.find((r) => r.name === "Me")!;
    expect(me.folder).toBe("Auth");
    expect(me.request.url).toBe("{{baseUrl}}/me");
    expect(me.request.auth).toMatchObject({ kind: "bearer", token: "{{token}}" });
  });
  it("maps form bodies and query parameters", () => {
    const login = bundle.requests.find((r) => r.name === "Login")!.request;
    expect(login).toMatchObject({ body_kind: "form", body: "user=kim" });
    const search = bundle.requests.find((r) => r.name === "Search")!.request;
    expect(search.params).toEqual([{ key: "q", value: "devbox" }]);
  });
  it("merges base values into sub environments and warns about tags", () => {
    expect(bundle.environments.map((e) => e.name)).toEqual(["Base Environment", "staging"]);
    expect(bundle.environments[1].variables).toEqual([{ key: "baseUrl", value: "https://staging.x.test" }, { key: "token", value: "t" }]);
    expect(bundle.warnings.some((w) => w.includes("템플릿 태그"))).toBe(true);
  });
});
```

- [ ] **Step 3: 실패 확인·구현·확인** — `_type`별로 모아 `parentId`로 폴더 경로를 만든다(workspace는 루트). 템플릿 `{{ _.x }}`·`{{ x }}` → `{{x}}`, `{% … %}` 태그는 그대로 두고 경고(요청 이름 포함). body `mimeType`: json → json(`text`), form-urlencoded → form(활성 `params`), multipart → multipart(텍스트 파트만), `application/graphql` → graphql(`text`의 JSON `{query, variables}`), 그 밖 → raw. `authentication` bearer·basic·apikey(`addTo: "header"`만, 아니면 경고). `parameters` → `params`(활성만). 환경: base(부모가 workspace)와 그 아래 sub 환경을 각각 만들고 sub에는 base 값을 먼저 넣은 뒤 덮는다(키 순서: sub 키, 그다음 base에만 있는 키). Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/requests/lib/importers/insomnia.test.ts` → PASS. 커밋: `git add -A && git commit -m "feat(devbox-api-studio): import Insomnia exports"`

---

### Task A4: HAR

**Files:** Create `lib/importers/{har.ts,har.test.ts}`

- [ ] **Step 1: 실패하는 테스트**

```ts
import { describe, expect, it } from "vitest";
import { parseHar } from "./har";

const har = (entries: unknown[]) => JSON.stringify({ log: { version: "1.2", entries } });
const entry = (method: string, url: string, extra: Record<string, unknown> = {}) => ({
  request: { method, url, headers: [{ name: ":authority", value: "x.test" }, { name: "Accept", value: "*/*" }, { name: "Content-Length", value: "2" }, { name: "Cookie", value: "a=1" }], ...extra },
  response: { status: 200 },
});

describe("HAR import", () => {
  it("keeps http requests, drops browser-managed headers and groups by host", () => {
    const bundle = parseHar(har([
      entry("GET", "https://x.test/api/users?page=1"),
      entry("POST", "https://x.test/api/users", { postData: { mimeType: "application/json", text: "{\"a\":1}" } }),
      entry("GET", "data:image/png;base64,AAAA"),
      entry("GET", "https://x.test/api/users?page=1"),
    ]));
    expect(bundle.requests.map((r) => `${r.folder}|${r.name}`)).toEqual(["x.test|GET /api/users", "x.test|POST /api/users"]);
    const [get, post] = bundle.requests.map((r) => r.request);
    expect(get.headers).toEqual([{ key: "Accept", value: "*/*", enabled: true }]);
    expect(get.cookies).toEqual([{ name: "a", value: "1", enabled: true }]);
    expect(post).toMatchObject({ body_kind: "json", body: "{\"a\":1}" });
    expect(bundle.warnings).toEqual(["같은 요청 1개와 http(s)가 아닌 요청 1개를 건너뛰었습니다."]);
  });
  it("caps very large archives", () => {
    const many = Array.from({ length: 510 }, (_, i) => entry("GET", `https://x.test/${i}`));
    const bundle = parseHar(har(many));
    expect(bundle.requests).toHaveLength(500);
    expect(bundle.warnings).toContain("요청이 많아 처음 500개만 가져왔습니다.");
  });
});
```

- [ ] **Step 2: 실패 확인·구현·확인** — `log.entries[].request`만 읽는다(응답 무시). 건너뛰는 헤더: `:`로 시작, `host`, `content-length`, `connection`, `accept-encoding`, `cookie`(쿠키로 옮김). `postData.mimeType`으로 본문 종류(json·form(`params` 또는 `text`)·multipart(텍스트 파라미터만)·raw). method+url+body가 같은 항목은 한 번만. 폴더는 host, 이름은 `<METHOD> <pathname>`. Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/requests/lib/importers/har.test.ts` → PASS. 커밋: `git add -A && git commit -m "feat(devbox-api-studio): import HAR archives"`

---

### Task A5: Bruno `.bru`

**Files:** Create `lib/importers/{bruno.ts,bruno.test.ts}`

**Interfaces (Produces):** `parseBruBlocks(text: string): BruBlock[]`(`{ name: string; kind: "dict" | "text" | "list"; entries?: { key: string; value: string; enabled: boolean }[]; text?: string; items?: string[] }`), `parseBruno(files: { relativePath: string; text: string }[]): ImportBundle`(요청 파일과 `environments/*.bru`를 함께 받는다)

- [ ] **Step 1: 실패하는 테스트**

```ts
import { describe, expect, it } from "vitest";
import { parseBruno, parseBruBlocks } from "./bruno";

const REQUEST = `meta {
  name: Create user
  type: http
  seq: 2
}

post {
  url: {{baseUrl}}/users
  body: json
  auth: bearer
}

headers {
  Accept: application/json
  ~X-Debug: 1
}

auth:bearer {
  token: {{token}}
}

body:json {
  {
    "name": "kim"
  }
}

script:pre-request {
  req.setHeader("x", "y");
}
`;

const ENV = `vars {
  baseUrl: https://x.test
}
vars:secret [
  token
]
`;

describe("Bruno import", () => {
  it("splits dictionary, text and list blocks", () => {
    const blocks = parseBruBlocks(REQUEST);
    expect(blocks.map((b) => b.name)).toEqual(["meta", "post", "headers", "auth:bearer", "body:json", "script:pre-request"]);
    expect(blocks.find((b) => b.name === "headers")?.entries).toEqual([
      { key: "Accept", value: "application/json", enabled: true },
      { key: "X-Debug", value: "1", enabled: false },
    ]);
    expect(blocks.find((b) => b.name === "body:json")?.text).toBe('{\n  "name": "kim"\n}');
    expect(parseBruBlocks(ENV).find((b) => b.name === "vars:secret")?.items).toEqual(["token"]);
  });

  it("builds requests with folders and environments with secret keys", () => {
    const bundle = parseBruno([
      { relativePath: "users/create-user.bru", text: REQUEST },
      { relativePath: "environments/local.bru", text: ENV },
    ]);
    expect(bundle.requests[0]).toMatchObject({ name: "Create user", folder: "users" });
    expect(bundle.requests[0].request).toMatchObject({ method: "POST", url: "{{baseUrl}}/users", body_kind: "json", body: '{\n  "name": "kim"\n}' });
    expect(bundle.requests[0].request.auth).toMatchObject({ kind: "bearer", token: "{{token}}" });
    expect(bundle.environments).toEqual([{ name: "local", variables: [{ key: "baseUrl", value: "https://x.test" }, { key: "token", value: "" }] }]);
    expect(bundle.warnings).toContain("스크립트·테스트 블록 1개는 가져오지 않았습니다.");
  });
});
```

- [ ] **Step 2: 실패 확인·구현·확인** — 블록은 줄 시작의 `이름 {`(dict·text)나 `이름 [`(list)로 열고 0열의 `}`/`]`로 닫는다. text 블록(`body:*`, `script:*`, `tests`, `docs`)은 내용 줄의 앞 2칸을 떼어 그대로, dict 블록은 `key: value`(첫 `:` 기준, `~` 접두사는 비활성), list 블록은 항목 줄. HTTP 블록 이름(`get`, `post`, `put`, `patch`, `delete`, `options`, `head`)이 방법, 그 안 `url`, `body`(none·json·text·xml·form-urlencoded·multipart-form·graphql), `auth`(none·bearer·basic·apikey). 본문: `body:json`→json, `body:text`·`body:xml`→raw, `body:form-urlencoded`→form, `body:multipart-form`→multipart(텍스트만), `body:graphql`+`body:graphql:vars`→graphql. 헤더 `headers`, 쿼리 `params:query`(옛 이름 `query`도). 폴더는 `relativePath`의 디렉터리. `environments/` 아래 파일은 환경: `vars` 값, `vars:secret` 키는 값 없이. Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/requests/lib/importers/bruno.test.ts` → PASS. 커밋: `git add -A && git commit -m "feat(devbox-api-studio): import Bruno requests"`

---

### Task A6: native 파일 읽기와 형식 감지

**Files:** Create `crates/http-client-engine/src/commands/import_files.rs`; Modify `crates/http-client-engine/src/{api.rs,component.rs}`(P1-13 `http_client_engine::api::ApiCall`, `METHODS`, `classify`), `apps/devbox-api-studio/src-tauri/tests/{typescript.rs,fixtures/…}`, `lib/importers/index.ts`(+`index.test.ts`), `requests/api.ts`

**Interfaces (Produces):** native 메서드 `read_import_files { format: "devbox" | "postman" | "insomnia" | "har" | "bruno" | "auto" }` → `Option<Vec<ImportFile { name: String, relative_path: String, text: String }>>`(취소면 `null`); 필터: devbox·postman·insomnia `["json"]`, har `["har","json"]`, bruno `["bru"]`(여러 개 선택), auto `["json","har","bru"]`. 오류 `import_file_invalid`, `import_file_too_large`. TS `readImportFiles(format)`, `detectFormat`

- [ ] **Step 1: 실패하는 테스트**
  - `import_files.rs`(순수 부분): `common_parent(paths)`로 여러 `.bru`의 공통 부모 기준 상대 경로를 만든다(`a/users/x.bru`, `a/y.bru` → `users/x.bru`, `y.bru`), `decode_utf8(bytes)`는 BOM을 떼고 잘못된 UTF-8은 `import_file_invalid`, `check_sizes(&[len])`는 파일 16MiB·합계 32MiB·개수 500 초과를 `import_file_too_large`로.
  - `index.test.ts`: `detectFormat("x.har", "{\"log\":{\"entries\":[]}}")` → `"har"`, `detectFormat("x.json", postmanText)` → `"postman"`, `detectFormat("x.json", insomniaText)` → `"insomnia"`, `detectFormat("x.json", devboxExportText)` → `"devbox"`, `detectFormat("a.bru", "meta {\n}")` → `"bruno"`, `detectFormat("x.json", "{}")` → `null`. `parseImport("postman", [{…, text: "{not json"}])`은 `ImportError("가져올 수 없는 형식입니다.")`.
- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-http-client-engine --lib import_files && pnpm --filter @devbox/api-studio-features exec vitest run src/requests/lib/importers/index.test.ts` → FAIL
- [ ] **Step 3: 구현** — `read_import_files`는 기존 `read_json_file`처럼 `app.dialog().file()`에 필터를 달고 bruno면 `blocking_pick_files`, 아니면 `blocking_pick_file`. 파일마다 기존 `read_bounded`(한도를 인자로 받도록 일반화)로 읽는다. `ApiCall`에 variant 추가(class는 기존 `read_json_file`과 같게), `METHODS` 목록 테스트(P1-13 Task 1)에 새 이름 추가, 생성 TS 갱신. `detectFormat`: 확장자 `.bru` → bruno, `.har` 또는 `log.entries` 배열 → har, `info.schema`에 `getpostman` → postman, `_type === "export"` → insomnia, 기존 Devbox export `schema` → devbox. `parseImport("devbox", …)`는 기존 `parseCollectionExport`/`parseEnvironmentExport` 결과를 번들 모양으로 감싼다.
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령과 `cargo test -p devbox-api-studio --test typescript && bash .github/scripts/check-generated-bindings.sh` → PASS. `git add -A && git commit -m "feat(devbox-api-studio): read import files natively"`

---

### Task A7: 가져오기 화면

**Files:** Create `packages/api-studio-features/src/requests/ImportDialog.tsx`, `ImportDialog.test.tsx`; Modify 컬렉션 영역 부품(P1-15에서 `App.tsx`에서 나눈 컬렉션 사이드바; 기존 "JSON 가져오기" 버튼 옆)

- [ ] **Step 1: 실패하는 테스트** — `ImportDialog.test.tsx`
  - "가져오기" → 대화상자에 "curl 붙여넣기"와 "파일에서" 두 가지. curl 텍스트를 붙여 넣고 "미리 보기" → 요청 1개 행("POST /users · https://api.example.com/users")과 경고 목록.
  - "파일에서" → 형식 선택(자동·Postman·Insomnia·HAR·Bruno·Devbox JSON) → `read_import_files` mock 결과로 미리 보기: 폴더별로 묶인 체크 목록(기본 모두 선택), "비밀 검토 필요" 표시, 환경 N개.
  - 몇 개를 해제하고 "선택한 N개 가져오기" → 컬렉션 저장소에 그 요청만 추가, 환경은 "환경도 가져오기" 체크가 켜져 있을 때만. 완료 후 "N개를 가져왔습니다." 토스트와 "되돌리기"(누르면 이전 문서로 되돌림).
  - 형식을 알 수 없거나 파싱 실패면 "가져올 수 없는 형식입니다." 한 줄, 저장소 변화 없음.
  - axe 위반 0.
- [ ] **Step 2: 실패 확인** — Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/requests/ImportDialog.test.tsx` → FAIL
- [ ] **Step 3: 구현** — 대화상자는 `<dialog>`가 아니라 기존 API Studio 패널 방식(같은 파일의 다른 확인 영역과 같은 모양)으로. 적용은 P1-17 `DocumentStorage`의 컬렉션·환경 문서를 revision과 함께 쓰고, 되돌리기는 이전 문서를 새 revision 조건으로 쓴다(그 사이 다른 변경이 있으면 "그 사이 바뀐 내용이 있어 되돌리지 않았습니다.").
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령과 `pnpm --filter @devbox/api-studio-features exec vitest run src/requests` → PASS. `git add -A && git commit -m "feat(devbox-api-studio): import preview dialog"`

---

### Task A8: PR 완료

- [ ] `docs/windows-guide.md` API Studio 절에 "가져오기" 문단(지원 형식, 비밀 처리, 스크립트 미지원).
- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): Chrome DevTools "Copy as cURL (bash)"·"(cmd)" 둘 다 붙여 넣어 같은 요청이 되는지, 실제 Postman·Insomnia 내보내기 파일 하나씩, 브라우저 HAR 하나.

---

## PR B — 파일 컬렉션

- 묶음: **B12** — 브랜치 `feat/devbox-api-studio/imports-runner-auth`, PR 제목 `feat(devbox-api-studio): imports, file collections, runner, OAuth 2.0, TLS and code generation`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `feat(devbox-api-studio): export and import collections as request files`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).
- 순서: 같은 묶음 안에서 PR A 과제를 모두 마친 뒤.

### Task B1: 파일 컬렉션 형식

**Files:** Create `packages/api-studio-features/src/requests/lib/fileCollection.ts`, `fileCollection.test.ts`

**Interfaces (Produces):** `FILE_COLLECTION_MARKER = "collection.devbox.json"`, `serializeFileCollection(name: string, store: CollectionStore): { relativePath: string; text: string }[]`, `parseFileCollection(files: { relativePath: string; text: string }[]): ImportBundle`(`.request.json`은 Devbox 형식, `.bru`는 Task A5 `parseBruno`로), `safeFileName(name: string): string`

- [ ] **Step 1: 실패하는 테스트**

```ts
import { describe, expect, it } from "vitest";
import { FILE_COLLECTION_MARKER, parseFileCollection, safeFileName, serializeFileCollection } from "./fileCollection";
import { emptyRequest } from "./importers";

const entry = (id: string, name: string, folder: string) => ({
  id, name, folder, saved_at: 1, requiresSecretReview: false,
  request: { ...emptyRequest(), url: `https://x.test/${id}`, requiresSecretReview: false },
});

describe("file collections", () => {
  it("writes one file per request under folder paths plus a marker", () => {
    const files = serializeFileCollection("Demo", { version: 2, collections: [entry("a", "List: users?", "Users/Admin"), entry("b", "List: users?", "Users/Admin"), entry("c", "Health", "")] });
    expect(files.map((f) => f.relativePath)).toEqual([
      FILE_COLLECTION_MARKER,
      "Users/Admin/List- users-.request.json",
      "Users/Admin/List- users- (2).request.json",
      "Health.request.json",
    ]);
    expect(JSON.parse(files[1].text)).toMatchObject({ schema: "devbox.api-studio.request", schemaVersion: 1, name: "List: users?" });
  });

  it("round-trips through parse and keeps folders from paths", () => {
    const files = serializeFileCollection("Demo", { version: 2, collections: [entry("a", "Health", "Ops")] });
    const bundle = parseFileCollection(files);
    expect(bundle.requests).toEqual([{ name: "Health", folder: "Ops", request: expect.objectContaining({ url: "https://x.test/a" }) }]);
  });

  it("file names avoid reserved characters, dots and device names", () => {
    expect(safeFileName("  a/b\\c:d*e?f\"g<h>i|j.  ")).toBe("a-b-c-d-e-f-g-h-i-j");
    expect(safeFileName("CON")).toBe("_CON");
    expect(safeFileName("")).toBe("untitled");
    expect(safeFileName("가".repeat(100)).length).toBeLessThanOrEqual(80);
  });
});
```

- [ ] **Step 2: 실패 확인·구현·확인** — 요청 파일 내용은 `cleanPersistedRequest`를 거친 요청(비밀은 이미 `[REDACTED]`/변수 참조)과 이름뿐이다. 폴더 문자열의 `/`는 디렉터리, 각 조각도 `safeFileName`. Windows 예약 이름(`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`, 대소문자 무시)은 앞에 `_`. JSON은 2칸 들여쓰기 + 끝 줄바꿈(git diff가 읽기 좋게). Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/requests/lib/fileCollection.test.ts` → PASS. 커밋: `git add -A && git commit -m "feat(devbox-api-studio): file collection format"`

---

### Task B2: native 폴더 읽기·쓰기

**Files:** Create `crates/http-client-engine/src/commands/collection_folder.rs`; Modify `api.rs`, `component.rs`, 생성 TS, `requests/api.ts`

**Interfaces (Produces):** native 메서드 `pick_collection_folder {}` → `Option<FolderGrant { grant_id: String, name: String }>`(폴더 이름만, 경로는 native에만), `read_collection_folder { grant_id }` → `Vec<ImportFile>`(`*.request.json`, `*.bru`, `environments/*.bru`, `collection.devbox.json`), `write_collection_folder { grant_id, files: Vec<{ relative_path, text }> }` → `WriteResult { written: u32, stale: Vec<String> }`; 오류 `folder_grant_expired`, `folder_not_collection`(비어 있지 않고 표시 파일도 없음), `folder_path_invalid`, `folder_too_large`

- [ ] **Step 1: 실패하는 테스트** — `collection_folder.rs`(grant 없이 경로를 직접 받는 내부 함수 `read_folder(root)`·`write_folder(root, files)`로)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn file(path: &str, text: &str) -> FolderFile { FolderFile { relative_path: path.into(), text: text.into() } }

    #[test]
    fn writes_only_into_empty_or_marked_folders_and_reports_stale_files() {
        let root = tempfile::tempdir().unwrap();
        let marker = file("collection.devbox.json", "{\"schema\":\"devbox.api-studio.file-collection\",\"schemaVersion\":1,\"name\":\"Demo\"}\n");
        let first = write_folder(root.path(), &[marker.clone(), file("Ops/Health.request.json", "{}\n"), file("Old.request.json", "{}\n")]).unwrap();
        assert_eq!((first.written, first.stale.len()), (3, 0));
        let second = write_folder(root.path(), &[marker, file("Ops/Health.request.json", "{\"v\":2}\n")]).unwrap();
        assert_eq!(second.stale, vec!["Old.request.json".to_string()]);
        assert_eq!(fs::read_to_string(root.path().join("Ops/Health.request.json")).unwrap(), "{\"v\":2}\n");

        let other = tempfile::tempdir().unwrap();
        fs::write(other.path().join("notes.txt"), "mine").unwrap();
        assert_eq!(write_folder(other.path(), &[file("a.request.json", "{}")]).unwrap_err(), "folder_not_collection");
    }

    #[test]
    fn rejects_escaping_paths_and_links() {
        let root = tempfile::tempdir().unwrap();
        for bad in ["../x.request.json", "/abs.request.json", "a/../../x.request.json", "C:\\x.request.json", "a.txt"] {
            assert_eq!(write_folder(root.path(), &[file(bad, "{}")]).unwrap_err(), "folder_path_invalid", "{bad}");
        }
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
            assert_eq!(write_folder(root.path(), &[file("linked/x.request.json", "{}")]).unwrap_err(), "folder_path_invalid");
        }
    }

    #[test]
    fn reads_request_bruno_and_environment_files_recursively() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("users/admin")).unwrap();
        fs::create_dir_all(root.path().join("environments")).unwrap();
        fs::write(root.path().join("users/admin/a.bru"), "meta {\n}\n").unwrap();
        fs::write(root.path().join("b.request.json"), "{}").unwrap();
        fs::write(root.path().join("environments/local.bru"), "vars {\n}\n").unwrap();
        fs::write(root.path().join("ignored.txt"), "x").unwrap();
        let mut paths: Vec<_> = read_folder(root.path()).unwrap().into_iter().map(|f| f.relative_path).collect();
        paths.sort();
        assert_eq!(paths, vec!["b.request.json", "environments/local.bru", "users/admin/a.bru"]);
    }
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-http-client-engine --lib collection_folder` → FAIL
- [ ] **Step 3: 구현** — 상대 경로: `/` 구분, 빈 조각·`.`·`..`·드라이브 문자·`\`·절대 경로 금지, 확장자는 `.request.json`·`.bru`·표시 파일만(쓰기는 `.request.json`과 표시 파일만). 쓰기 전 검사: 폴더가 비었거나 표시 파일의 `schema`가 맞아야 한다. 경로의 각 조각은 `symlink_metadata`로 링크가 아닌지 확인하고 없으면 만든다. 파일은 `devbox_filesystem::atomic_write`. `stale` = 폴더 안 `*.request.json` 중 이번 목록에 없는 것. 읽기: 깊이 8, 파일 1,000개, 파일 1MiB, 링크는 건너뜀. grant: `pick_collection_folder`가 `blocking_pick_folder` 결과를 128비트 난수 id(기존 `random_hex_128` 같은 도우미)로 10분 보관(최대 8개), 읽기·쓰기는 grant로만. `ApiCall` variant, `METHODS` 목록 테스트, 생성 TS 갱신.
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령과 `bash .github/scripts/check-generated-bindings.sh` → PASS. `git add -A && git commit -m "feat(devbox-api-studio): read and write collection folders"`

---

### Task B3: 화면

**Files:** Modify 컬렉션 영역 부품, `ImportDialog.tsx`(+test)

- [ ] **Step 1: 실패하는 테스트**
  - 컬렉션 메뉴 "폴더로 내보내기" → `pick_collection_folder` → `write_collection_folder`(표시 파일 + 요청 파일) → "요청 N개를 폴더에 저장했습니다." + `stale`가 있으면 "이 폴더에 더 이상 없는 요청 파일 N개가 남아 있습니다: …". `folder_not_collection`이면 "다른 파일이 있는 폴더입니다. 빈 폴더나 이전에 내보낸 폴더를 골라 주세요."
  - `ImportDialog`의 "폴더에서"(파일 컬렉션·Bruno 컬렉션 폴더) → `read_collection_folder` → `parseFileCollection` → 같은 미리 보기.
  - axe 위반 0.
- [ ] **Step 2: 실패 확인·구현·확인** — Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/requests` → PASS. 커밋: `git add -A && git commit -m "feat(devbox-api-studio): file collection export and import"`

---

### Task B4: PR 완료

- [ ] 가이드에 "파일 컬렉션" 문단(형식, 저장소에 커밋하는 방법, 비밀은 변수로).
- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): 컬렉션을 WSL 프로젝트 폴더(`\\wsl.localhost\…`)와 Windows 폴더에 내보내고 다시 가져오기, 실제 Bruno 컬렉션 폴더 가져오기.
