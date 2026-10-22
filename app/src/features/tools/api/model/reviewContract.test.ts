// @vitest-environment node
// TS 저장 결과(파일 바이트)와 데몬 비밀 재검토 판정(`crates/api/src/collections/reviews.rs` `needs_review`)의 계약.
// 이 시험이 만든 파일 텍스트를 fixture로 커밋하고, Rust 단위 시험이 같은 fixture를 읽어 판정한다.
// 형식이 바뀌면 이 시험이 먼저 실패한다: `UPDATE_REVIEW_CONTRACT=1`로 다시 만들고 Rust 시험을 함께 돌린다.
import { readFileSync, writeFileSync } from "node:fs";
import { expect, it } from "vitest";
import type { CollectionRoot } from "../../../../rpc/gen/rpc";
import { emptyDraft, type RequestDraft, row, setAuthKind, updateAuth } from "./draft";
import { parseRequestFile, serializeRequestFile } from "./requestFile";
import { toStored } from "./stored";

const FIXTURE = new URL("../../../../../../crates/api/tests/fixtures/review-contract.json", import.meta.url);
const personal: CollectionRoot = { kind: "personal" };
const project: CollectionRoot = { kind: "project", projectId: "p1" };

interface Case {
  case: string;
  root: CollectionRoot;
  draft: RequestDraft;
  extra?: Record<string, unknown>;
  review: boolean;
}

const base = (): RequestDraft => ({ ...emptyDraft(), method: "POST", url: "https://api.test/x" });
const file = (path: string) => ({
  id: "p",
  enabled: true,
  kind: "file" as const,
  name: "upload",
  value: "",
  path,
  fileName: "",
  contentType: "",
});

function cases(): Case[] {
  return [
    { case: "plain", root: project, draft: base(), review: false },
    {
      case: "direct secret is masked, not a reference",
      root: project,
      draft: { ...base(), headers: [row("Authorization", "Bearer typed-secret")] },
      review: false,
    },
    {
      case: "secret reference in a header",
      root: project,
      draft: { ...base(), headers: [row("Authorization", "Bearer {{secret:GH}}")] },
      review: true,
    },
    {
      case: "secret reference in a disabled header",
      root: project,
      draft: { ...base(), headers: [row("X-Off", "{{secret:GH}}", false)] },
      review: true,
    },
    {
      case: "secret reference in bearer auth",
      root: project,
      draft: updateAuth(setAuthKind(base(), "bearer"), "bearer", { token: "{{secret:GH}}" }),
      review: true,
    },
    {
      case: "multipart file part (project root drops the path)",
      root: project,
      draft: { ...base(), bodyKind: "multipart", bodies: { multipart: { parts: [file("/home/u/a.zip")] } } },
      review: true,
    },
    {
      case: "multipart file part (personal root keeps the path)",
      root: personal,
      draft: { ...base(), bodyKind: "multipart", bodies: { multipart: { parts: [file("/home/u/a.zip")] } } },
      review: true,
    },
    {
      case: "binary body path",
      root: personal,
      draft: { ...base(), bodyKind: "binary", bodies: { binary: { path: "/home/u/.ssh/id_ed25519" } } },
      review: true,
    },
    {
      case: "binary body without a path",
      root: project,
      draft: { ...base(), bodyKind: "binary", bodies: { binary: { path: "/home/u/blob" } } },
      review: false,
    },
    ...(["caPath", "clientCertPath", "clientKeyPath"] as const).map((key) => ({
      case: `tls ${key}`,
      root: project,
      draft: { ...base(), tls: { ...base().tls, [key]: "/home/u/tls.pem" } },
      review: true,
    })),
    {
      case: "stdio command kept in an unknown field",
      root: project,
      draft: base(),
      extra: { "x-mcp": { transport: "stdio", command: "node server.js" } },
      review: true,
    },
  ];
}

function generate(): string {
  const rows = cases().map((item) => {
    const raw = item.extra
      ? parseRequestFile(
          JSON.stringify({
            version: 1,
            name: item.case,
            request: toStored(base()),
            assertions: [],
            captures: [],
            ...item.extra,
          }),
        ).raw
      : undefined;
    const { text } = serializeRequestFile(
      { version: 1, name: item.case, request: toStored(item.draft), assertions: [], captures: [] },
      item.root,
      raw,
    );
    return { case: item.case, review: item.review, text };
  });
  return `${JSON.stringify(rows, null, 2)}\n`;
}

it("saved_files_match_the_daemon_review_contract", () => {
  const generated = generate();
  if (process.env.UPDATE_REVIEW_CONTRACT === "1") writeFileSync(FIXTURE, generated);
  // 바이트 대신 값으로 비교한다(Biome 형식과 무관). Rust가 읽는 것은 각 행의 `text` 문자열이다.
  expect(JSON.parse(readFileSync(FIXTURE, "utf8"))).toEqual(JSON.parse(generated));
  // 비밀 참조가 있는 경우는 TS 쪽에서도 참조를 그대로 둔다(가리면 데몬이 재검토를 못 함).
  const rows = JSON.parse(generated) as { case: string; review: boolean; text: string }[];
  for (const entry of rows.filter((r) => r.case.startsWith("secret reference"))) {
    expect(entry.text).toContain("{{secret:GH}}");
  }
  expect(rows.find((r) => r.case.startsWith("direct secret"))!.text).not.toContain("typed-secret");
});
