// v0.9.0 fileCollection 시험 5개의 적응(v1 파일 배치: 요청 하나 = `<폴더…>/<이름>.request.json`)과 모르는 필드 왕복.
import { describe, expect, it } from "vitest";
import {
  isSafeCollectionPath,
  MAX_REQUEST_FILE_BYTES,
  parseRequestFile,
  type RequestFile,
  requestFilePath,
  safeFileName,
  serializeRequestFile,
} from "./requestFile";
import type { StoredRequest } from "./stored";

const personal = { kind: "personal" } as const;
const project = { kind: "project", projectId: "p1" } as const;

function request(url = "https://x.test/a"): StoredRequest {
  return {
    method: "GET",
    url,
    query: [],
    headers: [],
    cookies: [],
    body: { kind: "none" },
    auth: { kind: "none" },
    tls: { verify: true, caPath: null, clientCertPath: null, clientKeyPath: null },
    timeoutMs: 30_000,
    followRedirects: true,
  };
}
const file = (name: string, extra: Partial<RequestFile> = {}): RequestFile => ({
  version: 1,
  name,
  request: request(),
  assertions: [],
  captures: [],
  ...extra,
});

describe("collection request files", () => {
  it("collection_files_round_trip_without_losing_unknown_fields", () => {
    const original = {
      version: 1,
      name: "Users",
      "x-team": { owner: "api", tags: ["a", "b"] },
      request: { ...request(), "x-extra": [1, { deep: true }] },
      assertions: [],
      captures: [],
    };
    const parsed = parseRequestFile(JSON.stringify(original));
    expect(parsed.file.name).toBe("Users");
    const edited = { ...parsed.file, request: { ...parsed.file.request, url: "https://x.test/b" } };
    const { text } = serializeRequestFile(edited, project, parsed.raw);
    const written = JSON.parse(text);
    expect(written["x-team"]).toEqual({ owner: "api", tags: ["a", "b"] });
    expect(written.request["x-extra"]).toEqual([1, { deep: true }]);
    expect(written.request.url).toBe("https://x.test/b");
    // 다시 읽고 써도 같은 바이트(결정적 직렬화).
    const again = parseRequestFile(text);
    expect(serializeRequestFile(again.file, project, again.raw).text).toBe(text);
    expect(text.endsWith("\n")).toBe(true);
  });

  it("stale_known_fields_are_not_resurrected", () => {
    const parsed = parseRequestFile(
      JSON.stringify({ ...file("R"), requiresSecretReview: true, masked: ["request.headers.0.value"] }),
    );
    const { requiresSecretReview: _review, masked: _masked, ...rest } = parsed.file;
    const written = JSON.parse(serializeRequestFile(rest, personal, parsed.raw).text);
    expect(written).not.toHaveProperty("requiresSecretReview");
    expect(written).not.toHaveProperty("masked");
  });

  it("file_names_are_deterministic_and_folders_kept", () => {
    const used = new Set<string>();
    expect([
      requestFilePath("List: users?", "Users/Admin", used),
      requestFilePath("List: users?", "Users/Admin", used),
      requestFilePath("Health", "", used),
    ]).toEqual([
      "Users/Admin/List- users-.request.json",
      "Users/Admin/List- users- (2).request.json",
      "Health.request.json",
    ]);
    const { text } = serializeRequestFile(file("List: users?"), personal);
    expect(JSON.parse(text)).toMatchObject({ version: 1, name: "List: users?" });
    expect(parseRequestFile(text).file.name).toBe("List: users?");
  });

  it("reserved_names_and_case_collisions_get_unique_paths", () => {
    expect(safeFileName('  a/b\\c:d*e?f"g<h>i|j.  ')).toBe("a-b-c-d-e-f-g-h-i-j");
    expect(safeFileName("CON.txt")).toBe("_CON.txt");
    expect(safeFileName("CON")).toBe("_CON");
    expect(safeFileName("lpt9")).toBe("_lpt9");
    expect(safeFileName("")).toBe("untitled");
    expect(safeFileName("가".repeat(100)).length).toBeLessThanOrEqual(80);
    const used = new Set<string>();
    expect(requestFilePath("Foo", "", used)).toBe("Foo.request.json");
    expect(requestFilePath("foo", "", used)).toBe("foo (2).request.json");
    // 폴더 조각에도 Windows 금지 이름·글자 규칙을 쓴다.
    expect(requestFilePath("x", "AUX/a:b", new Set())).toBe("_AUX/a-b/x.request.json");
  });

  it("unsafe_paths_malformed_and_oversized_files_are_rejected", () => {
    for (const path of [
      "../x.request.json",
      "a/./x.request.json",
      "a//x.request.json",
      "/abs.request.json",
      "a\\x.request.json",
      "C:x.request.json",
      "CON.request.json",
      "a/NUL/x.request.json",
      "a /x.request.json",
      "a./x.request.json",
      "x.request.txt",
      "x\u0001.request.json",
    ]) {
      expect(isSafeCollectionPath(path), path).toBe(false);
    }
    expect(isSafeCollectionPath("Users/Admin/List- users-.request.json")).toBe(true);
    expect(isSafeCollectionPath("env/dev.json")).toBe(true);
    expect(() => parseRequestFile("{}")).toThrow();
    expect(() => parseRequestFile("{broken")).toThrow();
    expect(() => parseRequestFile(JSON.stringify({ ...file("x"), version: 2 }))).toThrow();
    expect(() => parseRequestFile(JSON.stringify({ ...file("x"), request: { url: 1 } }))).toThrow();
    expect(() => parseRequestFile(" ".repeat(MAX_REQUEST_FILE_BYTES + 1))).toThrow();
    const huge = file("big", {
      request: { ...request(), body: { kind: "json", text: "x".repeat(MAX_REQUEST_FILE_BYTES) } },
    });
    expect(() => serializeRequestFile(huge, personal)).toThrow();
  });

  it("secrets_are_redacted_before_serialization_and_review_flag_survives", () => {
    const secret = file("Secret", {
      request: {
        ...request("https://x.test?token=secret"),
        headers: [{ name: "Authorization", value: "Bearer raw", enabled: true }],
      },
      requiresSecretReview: true,
    });
    const { text, masked } = serializeRequestFile(secret, project);
    expect(text).not.toContain("token=secret");
    expect(text).not.toContain("Bearer raw");
    expect(masked).toEqual(["request.url", "request.headers.0.value"]);
    const reopened = parseRequestFile(text);
    expect(reopened.file.requiresSecretReview).toBe(true);
    expect(reopened.file.masked).toEqual(["request.url", "request.headers.0.value"]);
    // 가린 값을 다시 입력하지 않고 다시 저장해도 위치가 남는다(다시 열어도 송신을 막음).
    const resaved = serializeRequestFile(reopened.file, project, reopened.raw);
    expect(JSON.parse(resaved.text).masked).toEqual(["request.url", "request.headers.0.value"]);
    // 다시 입력해 가린 값이 없어지면 그 위치는 지운다.
    const fixed = {
      ...reopened.file,
      request: { ...reopened.file.request, url: "https://x.test?token={{secret:T}}", headers: [] },
    };
    expect(JSON.parse(serializeRequestFile(fixed, project, reopened.raw).text)).not.toHaveProperty("masked");
  });

  it("unicode_names_fit_filename_limits", () => {
    const used = new Set<string>();
    const paths = [requestFilePath("가".repeat(80), "", used), requestFilePath("가".repeat(80), "", used)];
    expect(new Set(paths).size).toBe(2);
    for (const path of paths) {
      for (const segment of path.split("/")) expect(new TextEncoder().encode(segment).length).toBeLessThanOrEqual(255);
      expect(path.endsWith(".request.json")).toBe(true);
    }
    expect(requestFilePath("é", "", new Set())).toBe("é.request.json");
  });
});
