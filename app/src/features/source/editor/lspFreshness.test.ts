import { afterEach, expect, test } from "vitest";
import { keyId, memoryJournal } from "../../documents/offlineJournal";
import { useDocuments } from "../../documents/store";
import { FakeDocumentsDaemon } from "../../documents/test-daemon";
import { useSource } from "../store";
import { capturedContext } from "./lspProof.fixture";
import { applyWorkspaceEdit, projectUris } from "./lspWorkspace";
import { emptyTabs } from "./tabs";

const a = { root: "project:p", path: "a.ts" };
const b = { root: "project:p", path: "b.ts" };
const uris = projectUris("p", "/owned/pre-s6");
const oldText = "const foo = 1;";
const newerText = "const bar = 0; const foo = 1;";
const edit = {
  range: { start: { line: 0, character: 6 }, end: { line: 0, character: 9 } },
  newText: "renamed",
};
let stop = () => {};
afterEach(() => {
  stop();
  useSource.setState({ layout: emptyTabs(), recent: [], locations: {} });
});
async function setup(openTarget = false) {
  const rpc = new FakeDocumentsDaemon();
  rpc.write(a, "foo();");
  rpc.write(b, oldText);
  stop = useDocuments.getState().connect(rpc, { journal: memoryJournal(), windowId: "pre-s6" });
  useDocuments.getState().open(a, { kind: "code" });
  if (openTarget) useDocuments.getState().open(b, { kind: "code" });
  await useDocuments.getState().settled();
  // This is the exact production snapshot taken before awaiting textDocument/rename.
  const snapshot = await capturedContext(rpc, uris, [b]);
  return { rpc, snapshot };
}

test.each(["external disk", "newly opened local buffer"])(
  "delayed unversioned rename refuses stale offsets in %s target absent at request time",
  async (kind) => {
    const { rpc, snapshot } = await setup();
    expect(snapshot.expected?.has(keyId(b))).toBe(false);
    if (kind === "external disk") rpc.write(b, newerText);
    else {
      useDocuments.getState().open(b, { kind: "code" });
      await useDocuments.getState().settled();
      useDocuments.getState().edit(b, newerText);
    }
    // The delayed response is valid LSP changes (unversioned) computed from oldText.
    await expect(applyWorkspaceEdit(snapshot, { changes: { [uris.uri(b)]: [edit] } })).rejects.toThrow();
    expect(useDocuments.getState().find(b)?.text).toBe(newerText);
    expect(rpc.count("documents.save")).toBe(0);
  },
);

test("CONTROL: a target already open at request time rejects a later local edit", async () => {
  const { rpc, snapshot } = await setup(true);
  useDocuments.getState().edit(b, newerText);
  await expect(applyWorkspaceEdit(snapshot, { changes: { [uris.uri(b)]: [edit] } })).rejects.toThrow();
  expect(useDocuments.getState().find(b)?.text).toBe(newerText);
  expect(rpc.count("documents.save")).toBe(0);
});

test("CONTROL: unopened target with explicit unmapped document version is rejected", async () => {
  const { rpc, snapshot } = await setup();
  rpc.write(b, newerText);
  await expect(
    applyWorkspaceEdit(snapshot, {
      documentChanges: [{ textDocument: { uri: uris.uri(b), version: 1 }, edits: [edit] }],
    }),
  ).rejects.toThrow();
  expect(useDocuments.getState().find(b)?.text).toBe(newerText);
  expect(rpc.count("documents.save")).toBe(0);
});

test("unchanged unopened target applies with the request's daemon baseline", async () => {
  const { rpc, snapshot } = await setup();
  await applyWorkspaceEdit(snapshot, { changes: { [uris.uri(b)]: [edit] } });
  expect(useDocuments.getState().find(b)?.text).toBe("const renamed = 1;");
  expect(rpc.count("documents.save")).toBe(0);
  expect(snapshot.validate).toHaveBeenCalledOnce();
  expect(snapshot.finish).toHaveBeenCalledOnce();
});

test("dirty opened A and unchanged unopened B use distinct buffer and disk baselines", async () => {
  const { rpc } = await setup();
  useDocuments.getState().edit(a, "const foo = 2;");
  const snapshot = await capturedContext(rpc, uris, [a, b]);
  await applyWorkspaceEdit(snapshot, { changes: { [uris.uri(a)]: [edit], [uris.uri(b)]: [edit] } });
  expect(useDocuments.getState().find(a)?.text).toBe("const renamed = 2;");
  expect(useDocuments.getState().find(b)?.text).toBe("const renamed = 1;");
  expect(rpc.files.get(keyId(a))?.text).toBe("foo();");
  expect(rpc.count("documents.save")).toBe(0);
});

test.each([
  "missing coverage",
  "stale second target",
  "deleted second target",
  "replaced identity",
  "validation denied",
  "closing during validation",
])("%s refuses the whole edit before first buffer mutation", async (kind) => {
  const { rpc } = await setup();
  const snapshot = await capturedContext(rpc, uris, [a, b]);
  const before = useDocuments.getState().find(a)?.text;
  if (kind === "missing coverage") snapshot.proof!.targets.pop();
  if (kind === "stale second target") rpc.write(b, newerText);
  if (kind === "deleted second target") rpc.files.delete(keyId(b));
  if (kind === "replaced identity") rpc.write(b, oldText);
  if (kind === "validation denied") snapshot.validate = async () => false;
  if (kind === "closing during validation")
    snapshot.validate = async () => {
      useDocuments.setState({ windowClosing: true });
      return true;
    };
  const aEdit = { ...edit, range: { start: { line: 0, character: 0 }, end: { line: 0, character: 3 } } };
  await expect(
    applyWorkspaceEdit(snapshot, { changes: { [uris.uri(a)]: [aEdit], [uris.uri(b)]: [edit] } }),
  ).rejects.toThrow();
  expect(useDocuments.getState().find(a)?.text).toBe(before);
  expect(rpc.count("documents.save")).toBe(0);
  expect(snapshot.finish).toHaveBeenCalledOnce();
});

test("unversioned unsolicited edit cannot borrow a command snapshot", async () => {
  const { snapshot } = await setup(true);
  delete snapshot.proof;
  snapshot.unsolicited = true;
  await expect(applyWorkspaceEdit(snapshot, { changes: { [uris.uri(b)]: [edit] } })).rejects.toThrow();
  expect(useDocuments.getState().find(b)?.text).toBe(oldText);
});

test("versioned owned target retains its mapped version control", async () => {
  const { snapshot } = await setup(true);
  await applyWorkspaceEdit(snapshot, {
    documentChanges: [{ textDocument: { uri: uris.uri(b), version: 0 }, edits: [edit] }],
  });
  expect(useDocuments.getState().find(b)?.text).toBe("const renamed = 1;");
});

test("normalized BOM/CRLF text uses its own hash while raw disk identity stays distinct", async () => {
  const { rpc } = await setup();
  rpc.write(b, "\uFEFFconst foo = 1;\r\n");
  const rawRev = rpc.files.get(keyId(b))!.rev;
  // documents.read returns normalized text with the raw-file revision.
  rpc.files.get(keyId(b))!.text = `${oldText}\n`;
  const snapshot = await capturedContext(rpc, uris, [b]);
  expect(snapshot.proof!.targets[0]!.textHash).not.toBe(rawRev.hash);
  await applyWorkspaceEdit(snapshot, { changes: { [uris.uri(b)]: [edit] } });
  expect(useDocuments.getState().find(b)?.text).toBe("const renamed = 1;\n");
  expect(rpc.files.get(keyId(b))!.rev).toBe(rawRev);
});
