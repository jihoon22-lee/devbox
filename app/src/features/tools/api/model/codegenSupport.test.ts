import { describe, expect, it } from "vitest";
import { CODE_TARGETS, generateCode } from "../lib/codegen";
import { emptyDraft, setBodyKind, toSnapshot, updateBody } from "./draft";
import { draftFromFile, fileFromDraft } from "./fileDraft";
import { parseRequestFile, serializeRequestFile } from "./requestFile";
import { toStored } from "./stored";
import { templateFromDraft } from "./toTemplate";

const base = () => ({ ...emptyDraft(), method: "POST", url: "https://owned.invalid/upload" });
const privatePath = "/owned/PRIVATE_FILE_CANARY.bin";
const binary = () => updateBody(setBodyKind(base(), "binary"), "binary", { path: privatePath });
const encoded = () =>
  updateBody(setBodyKind(base(), "raw"), "raw", {
    text: "AP+A",
    encoding: "base64",
    contentType: "application/octet-stream",
  });

it("active binary upload explicitly rejects code generation instead of dropping its body", () => {
  expect(() => templateFromDraft(binary())).toThrow(/파일 본문의 코드 생성은 아직 지원하지 않습니다/);
});
it("active base64 raw explicitly rejects code generation instead of sending encoded characters", () => {
  expect(() => templateFromDraft(encoded())).toThrow(/Base64 원시 본문의 코드 생성은 아직 지원하지 않습니다/);
});
it.each(["caPath", "clientCertPath", "clientKeyPath"] as const)(
  "custom TLS %s explicitly rejects code generation",
  (field) => {
    const draft = { ...base(), tls: { ...base().tls, [field]: privatePath } };
    expect(() => templateFromDraft(draft)).toThrow(
      /사용자 TLS 파일이 설정된 요청의 코드 생성은 아직 지원하지 않습니다/,
    );
  },
);

it("inactive file/base64 drafts do not block supported bodies or the six generators", () => {
  let draft = updateBody(binary(), "raw", {
    text: "AP+A",
    encoding: "base64",
    contentType: "application/octet-stream",
  });
  draft = updateBody(setBodyKind(draft, "json"), "json", { text: '{"owned":"supported"}' });
  const template = templateFromDraft(draft);
  expect(template.body_kind).toBe("json");
  for (const target of CODE_TARGETS) {
    const code = generateCode(target, template, []).code;
    expect(code).toContain("supported");
    expect(code).not.toContain(privatePath);
    expect(code).not.toContain("AP+A");
  }
});
it("verify=false without custom TLS files keeps existing target behavior", () => {
  const draft = { ...base(), tls: { verify: false, caPath: "", clientCertPath: null, clientKeyPath: "" } };
  const template = templateFromDraft(draft);
  expect(template.tls).toEqual({ credentialId: null, verify: false });
  expect(generateCode("curl", template, []).code).toContain("--insecure");
  // Browser fetch retains its existing explicit unsupported execution guard.
  expect(generateCode("fetch", template, []).code).toContain("브라우저 fetch는 사용자 TLS 설정을 지원하지 않습니다");
});

describe.each([
  ["binary", binary],
  ["raw-base64", encoded],
] as const)("%s remains available outside code generation", (_name, make) => {
  it("preserves active payload and custom TLS through send, storage and request-file reload", () => {
    const draft = {
      ...make(),
      tls: { ...base().tls, caPath: privatePath, clientCertPath: "/owned/cert.pem", clientKeyPath: "/owned/key.pem" },
    };
    const snapshot = toSnapshot(draft);
    const stored = toStored(draft);
    expect(snapshot.body).toEqual(stored.body);
    expect(snapshot.tls).toEqual(draft.tls);
    const saved = serializeRequestFile(fileFromDraft(draft, "owned"), { kind: "personal" });
    const reloaded = draftFromFile(parseRequestFile(saved.text));
    expect(toSnapshot(reloaded).body).toEqual(snapshot.body);
    expect(toStored(reloaded).tls).toEqual(stored.tls);
  });
});
