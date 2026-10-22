import { describe, expect, it, vi } from "vitest";
import type { RequestSnapshot } from "../../../../rpc/gen/rpc";
import { variableUses } from "../runner/plan";
import { emptyDraft, row, secretNames, setAuthKind, setBodyKind, toSnapshot, updateAuth, updateBody } from "./draft";
import { serializeRequestFile } from "./requestFile";
import { toStored } from "./stored";

const personal = { kind: "personal" } as const;

describe("request draft", () => {
  // AR-F06 회귀(07b 회귀 표): 종류를 바꿔도 각 종류 초안이 그대로, 보내기·저장은 활성 종류만.
  it("body_kind_switch_keeps_each_draft", async () => {
    let draft = setBodyKind({ ...emptyDraft(), method: "POST", url: "https://api.test/graphql" }, "graphql");
    draft = updateBody(draft, "graphql", {
      query: "query Q { me { id } }",
      variables: '{"a":1}',
      operationName: "Q",
    });
    draft = setBodyKind(setBodyKind(draft, "none"), "graphql");
    expect(draft.bodyKind).toBe("graphql");
    expect(draft.bodies.graphql).toMatchObject({
      query: "query Q { me { id } }",
      variables: '{"a":1}',
      operationName: "Q",
    });

    draft = updateBody(setBodyKind(draft, "json"), "json", { text: '{"x":1}' });
    draft = updateBody(setBodyKind(draft, "graphql"), "graphql", {
      query: "query Q { me { id name } }",
      variables: '{"token":"{{secret:MISSING}}"}',
    });
    draft = setBodyKind(draft, "json");
    expect(draft.bodies.json).toEqual({ text: '{"x":1}' });
    expect(draft.bodies.graphql).toMatchObject({
      query: "query Q { me { id name } }",
      variables: '{"token":"{{secret:MISSING}}"}',
      operationName: "Q",
    });

    const snapshot = toSnapshot(draft);
    expect(snapshot.body).toEqual({ kind: "json", text: '{"x":1}' });
    expect(toStored(draft).body).toEqual({ kind: "json", text: '{"x":1}' });
    const { text } = serializeRequestFile(
      { version: 1, name: "Q", request: toStored(draft), assertions: [], captures: [] },
      personal,
    );
    expect(JSON.parse(text).request.body).toEqual({ kind: "json", text: '{"x":1}' });
    expect(text).not.toContain("query Q");
    expect(text).not.toContain("MISSING");
    expect(secretNames(snapshot)).not.toContain("MISSING");

    // 가짜 send: 해석할 수 없는 비밀이 있으면 거절한다(데몬 `api.secret_missing`과 같은 판정).
    const known = new Set<string>();
    const send = vi.fn(async (sent: RequestSnapshot) => {
      const missing = secretNames(sent).filter((name) => !known.has(name));
      if (missing.length) throw new Error(`secret_missing: ${missing.join(",")}`);
      return 200;
    });
    await expect(send(snapshot)).resolves.toBe(200);
    expect(send).toHaveBeenCalledOnce();
    // 활성으로 돌리면 그 비밀을 쓴다(비활성일 때만 무시).
    expect(secretNames(toSnapshot(setBodyKind(draft, "graphql")))).toEqual(["MISSING"]);
  });

  it("auth_kind_switch_keeps_each_draft_and_ignores_inactive_secrets", () => {
    let draft = updateAuth(setAuthKind(emptyDraft(), "bearer"), "bearer", { token: "{{secret:INACTIVE}}" });
    draft = updateAuth(setAuthKind(draft, "basic"), "basic", { username: "u", password: "{{secret:PASS}}" });
    draft = setAuthKind(draft, "none");
    expect(draft.auths.bearer).toEqual({ token: "{{secret:INACTIVE}}" });
    expect(draft.auths.basic).toEqual({ username: "u", password: "{{secret:PASS}}" });
    expect(toSnapshot(draft).auth).toEqual({ kind: "none" });
    expect(secretNames(toSnapshot(draft))).toEqual([]);
    draft = setAuthKind(draft, "basic");
    expect(toSnapshot(draft).auth).toEqual({ kind: "basic", username: "u", password: "{{secret:PASS}}" });
    expect(toStored(draft).auth).toEqual({ kind: "basic", username: "u", password: "{{secret:PASS}}" });
    expect(secretNames(toSnapshot(draft))).toEqual(["PASS"]);
  });

  it("snapshot_keeps_only_active_rows_with_screen_row_numbers", () => {
    const draft = {
      ...emptyDraft(),
      url: "https://api.test/{{path}}",
      query: [row("q", "1"), row("off", "{{hidden}}", false)],
      headers: [row("X-Off", "{{secret:OFF}}", false), row("X-On", "{{secret:ON}}")],
      cookies: [row("a", "1", false), row("b", "2")],
    };
    const snapshot = toSnapshot(draft);
    expect(snapshot.query).toEqual([{ name: "q", value: "1" }]);
    expect(snapshot.headers).toEqual([{ row: 1, name: "X-On", value: "{{secret:ON}}" }]);
    expect(snapshot.cookies).toEqual([{ row: 1, name: "b", value: "2" }]);
    expect(secretNames(snapshot)).toEqual(["ON"]);
    expect(variableUses(snapshot)).toEqual(["path"]);
    // 저장은 비활성 행을 그대로 남긴다(사용자가 쓴 행).
    expect(toStored(draft).headers).toEqual([
      { name: "X-Off", value: "{{secret:OFF}}", enabled: false },
      { name: "X-On", value: "{{secret:ON}}", enabled: true },
    ]);
  });
});
