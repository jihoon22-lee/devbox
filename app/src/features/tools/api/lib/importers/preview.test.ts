import { describe, expect, it } from "vitest";
import { MAX_EXPORTED_COLLECTIONS } from "../limits";
import { emptyRequest, ImportError, toImportPreview } from "./index";
import { isSafePreviewEnvironment } from "./preview";

// 적응: 미리보기 결과는 v0.9.0 내보내기 문서가 아니라 "만들 요청·환경 목록"이다(경로·직렬화는 3c requestFile).
describe("import preview", () => {
  it("redacts auth and blanks secret environment values in the files to create", () => {
    const preview = toImportPreview({
      requests: [
        {
          name: "Me",
          folder: "Users",
          request: {
            ...emptyRequest(),
            url: "https://x.test/me",
            auth: {
              kind: "bearer",
              username: "",
              password: "",
              token: "ghp_abcdefghijklmnopqrstuvwxyz0123",
              api_key: "",
              api_value: "",
            },
          },
        },
      ],
      environments: [
        {
          name: "dev",
          variables: [
            { key: "baseUrl", value: "https://x.test" },
            { key: "apiToken", value: "abc" },
          ],
        },
      ],
      warnings: [],
    });
    const [first] = preview.requests;
    expect(first?.request.auth?.token).toBe("[REDACTED]");
    expect(first?.requiresSecretReview).toBe(true);
    expect(first?.request.requiresSecretReview).toBe(true);
    expect(first?.folder).toBe("Users");
    expect(preview.environments[0]?.variables).toEqual([
      { key: "baseUrl", value: "https://x.test", secret: false },
      { key: "apiToken", value: "", secret: true },
    ]);
    // v0.9.0 `parseEnvironmentExport(...)` not null과 같은 의도: 미리보기 환경이 내보내기 검증 규칙을 통과한다.
    expect(preview.environments.every(isSafePreviewEnvironment)).toBe(true);
    expect(JSON.stringify(preview)).not.toContain("ghp_abcdefghijklmnopqrstuvwxyz0123");
    expect(JSON.stringify(preview)).not.toContain('"abc"');
  });
  it("bounds preview requests and reports truncation", () => {
    const preview = toImportPreview({
      requests: Array.from({ length: MAX_EXPORTED_COLLECTIONS + 1 }, () => ({
        name: "x",
        folder: "",
        request: { ...emptyRequest(), url: "https://x.test" },
      })),
      environments: [],
      warnings: [],
    });
    expect(preview.requests).toHaveLength(MAX_EXPORTED_COLLECTIONS);
    expect(preview.warnings.join(" ")).toContain(String(MAX_EXPORTED_COLLECTIONS));
  });
});

// 리뷰 3a-B1: v0.9.0 환경 검증(키 형식·토큰 모양 키·64KiB·제어 문자·U+2028·중복 키)을 지나지 못하면 가져오지 않는다.
it("rejects unsafe environment keys and values instead of storing them in plain text", () => {
  const preview = (variables: { key: string; value: string; secret?: boolean }[]) => () =>
    toImportPreview({ requests: [], environments: [{ name: "dev", variables }], warnings: [] });
  for (const variables of [
    [{ key: "a\nb", value: "x" }],
    [{ key: "ghp_abcdefghijklmnopqrstuvwx", value: "v" }],
    [{ key: "a b", value: "v" }],
    [{ key: "", value: "v" }],
    [{ key: "k".repeat(129), value: "v" }],
    [
      { key: "dup", value: "1" },
      { key: "dup", value: "2" },
    ],
    [{ key: "ok", value: "line\u0000" }],
    [{ key: "ok", value: "line\u2028sep" }],
    [{ key: "big", value: "x".repeat(64 * 1024 + 1) }],
  ])
    expect(preview(variables), JSON.stringify(variables).slice(0, 80)).toThrow(ImportError);
  const ok = toImportPreview({
    requests: [],
    environments: [
      { name: "eyJaaaaaaaaaaaa.bbbbbbbbbbbb.cccccccccccc env", variables: [{ key: "a.b-c_1", value: "x" }] },
    ],
    warnings: [],
  });
  expect(ok.environments[0]?.name).toBe("[REDACTED] env");
  expect(ok.environments.every(isSafePreviewEnvironment)).toBe(true);
  expect(
    isSafePreviewEnvironment({ name: "dev", variables: [{ key: "apiToken", value: "plain", secret: false }] }),
  ).toBe(false);
  expect(isSafePreviewEnvironment({ name: "dev", variables: [{ key: "t", value: "kept", secret: true }] })).toBe(false);
});
