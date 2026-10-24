import { expect, test } from "vitest";
import { closedDoc, type Doc } from "../documents/machine";
import { saveLabel } from "./saveLabel";

const key = { root: "vault", path: "a.md" };
const at0905 = `${BigInt(new Date(2026, 9, 24, 9, 5, 30).getTime()) * 1_000_000n}`;
const rev = { hash: "h", size: 1, mtimeNs: at0905, ino: "1" };
const doc = (patch: Partial<Doc>): Doc => ({
  ...closedDoc("d", key, { kind: "note" }),
  leaseId: "L1",
  diskRev: rev,
  ...patch,
});

test("the four save states have their own words", () => {
  expect(saveLabel(doc({ state: "Clean" }), true)).toBe("저장됨 09:05");
  expect(saveLabel(doc({ state: "Saving", editSeq: 2, savedSeq: 1 }), true)).toBe("저장 중");
  expect(saveLabel(doc({ state: "Dirty", editSeq: 2, savedSeq: 1 }), true)).toBe("저장 중");
  expect(saveLabel(doc({ state: "Dirty", editSeq: 2, savedSeq: 1, retry: true, leaseId: undefined }), false)).toBe(
    "연결 끊김 · 이 기기에 임시 저장",
  );
  expect(saveLabel(doc({ state: "Dirty", editSeq: 2, savedSeq: 1, error: "connection_lost" }), true)).toBe(
    "연결 끊김 · 이 기기에 임시 저장",
  );
  expect(saveLabel(doc({ state: "Conflict", editSeq: 2, savedSeq: 1 }), true)).toBe("충돌 · 비교 필요");
});

test("a clean note while offline is still saved; states shown elsewhere have no label", () => {
  expect(saveLabel(doc({ state: "Clean" }), false)).toBe("저장됨 09:05");
  expect(saveLabel(doc({ state: "Clean", diskRev: null }), true)).toBe("저장됨");
  expect(saveLabel(doc({ state: "ReadOnly" }), true)).toBeNull();
  expect(saveLabel(doc({ state: "Recovering" }), true)).toBeNull();
  expect(saveLabel(doc({ state: "Loading" }), true)).toBeNull();
});

test("manual code buffers say unsaved until a save starts", () => {
  expect(saveLabel(doc({ policy: { kind: "code" }, state: "Dirty", editSeq: 1 }), true)).toBe("저장하지 않음 · Ctrl+S");
  expect(saveLabel(doc({ policy: { kind: "code" }, state: "Saving", editSeq: 1 }), true)).toBe("저장 중");
});
