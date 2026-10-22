// 검토 권장(3c): 가져오기 미리보기가 이미 가린 값도 `masked`로 기록하고, 재검토 표시를 합쳐 실행기 항목을 만든다.
import { describe, expect, it } from "vitest";
import { emptyRequest } from "../lib/importers";
import { toImportPreview } from "../lib/importers/preview";
import { draftFromFile } from "./fileDraft";
import { importedFile, reviewNeeded, runEntryFromFile, storedFromTemplate } from "./fromTemplate";
import { redactedFields } from "./masked";
import { parseRequestFile, serializeRequestFile } from "./requestFile";

const personal = { kind: "personal" } as const;
const project = { kind: "project", projectId: "p" } as const;

describe("imported request files", () => {
  it("preview_masks_are_recorded_and_block_sending", () => {
    const preview = toImportPreview({
      requests: [
        {
          name: "Me",
          folder: "",
          request: {
            ...emptyRequest(),
            url: "https://x.test/me?access_token=raw-query",
            headers: [{ key: "Authorization", value: "Bearer raw-secret", enabled: true }],
          },
        },
      ],
      environments: [],
      warnings: [],
    });
    const file = importedFile(preview.requests[0]!);
    expect(file.masked).toEqual(["request.url", "request.headers.0.value"]);
    const { text } = serializeRequestFile(file, project);
    expect(text).not.toMatch(/raw-secret|raw-query/);
    const parsed = parseRequestFile(text);
    expect(parsed.file.masked).toEqual(["request.url", "request.headers.0.value"]);
    expect(redactedFields(draftFromFile(parsed))).toEqual(["request.url", "request.headers.0.value"]);
  });

  it("templates_convert_to_the_active_stored_shape", () => {
    const stored = storedFromTemplate({
      ...emptyRequest(),
      method: "POST",
      url: "https://x.test",
      params: [{ key: "q", value: "1" }],
      cookies: [{ name: "c", value: "{{C}}", enabled: false }],
      body_kind: "form",
      body: "a=1\nb={{B}}",
      auth: { kind: "bearer", username: "", password: "", token: "{{secret:T}}", api_key: "", api_value: "" },
    });
    expect(stored).toMatchObject({
      query: [{ name: "q", value: "1", enabled: true }],
      cookies: [{ name: "c", value: "{{C}}", enabled: false }],
      body: {
        kind: "form",
        fields: [
          { name: "a", value: "1", enabled: true },
          { name: "b", value: "{{B}}", enabled: true },
        ],
      },
      auth: { kind: "bearer", token: "{{secret:T}}" },
    });
    expect(
      storedFromTemplate({
        ...emptyRequest(),
        method: "GET",
        body_kind: "graphql",
        graphql: { query: "{ me { id } }", variables: "", operation_name: "" },
      }).body,
    ).toEqual({ kind: "graphql", query: "{ me { id } }", variables: "", operationName: null, method: "get" });
  });

  it("review_needed_combines_the_daemon_digest_and_the_file_flag", () => {
    const base = parseRequestFile(
      serializeRequestFile(
        importedFile(
          toImportPreview({
            requests: [{ name: "A", folder: "", request: { ...emptyRequest(), url: "https://x.test" } }],
            environments: [],
            warnings: [],
          }).requests[0]!,
        ),
        personal,
      ).text,
    ).file;
    expect(reviewNeeded({ reviewed: true }, base)).toBe(false);
    expect(reviewNeeded({ reviewed: false }, base)).toBe(true);
    expect(reviewNeeded({ reviewed: true }, { ...base, requiresSecretReview: true })).toBe(true);
    const parsed = parseRequestFile(
      serializeRequestFile(
        {
          ...base,
          captures: [
            { id: "c", enabled: true, variable: "token", source: "jsonPath", target: "$.t" },
            { id: "d", enabled: false, variable: "off", source: "status", target: "" },
          ],
        },
        personal,
      ).text,
    );
    expect(runEntryFromFile("a/A.request.json", { reviewed: false }, parsed)).toMatchObject({
      id: "a/A.request.json",
      name: "A",
      reviewNeeded: true,
      produces: ["token"],
    });
  });
});
