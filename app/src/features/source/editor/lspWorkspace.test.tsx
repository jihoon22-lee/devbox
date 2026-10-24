import { undo } from "@codemirror/commands";
import { EditorView } from "@codemirror/view";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, expect, test } from "vitest";
import { EditorHost } from "../../documents/EditorHost";
import { memoryJournal } from "../../documents/offlineJournal";
import { useDocuments } from "../../documents/store";
import { FakeDocumentsDaemon } from "../../documents/test-daemon";
import { applyWorkspaceEdit, projectUris } from "./lspWorkspace";

const a = { root: "project:p", path: "src/a.ts" },
  b = { ...a, path: "src/b.ts" };
const uris = projectUris("p", "/owned/한 project");
const edit = (text: string) => ({
  range: { start: { line: 0, character: 2 }, end: { line: 0, character: 3 } },
  newText: text,
});
let stop = () => {};
beforeAll(() =>
  Object.defineProperties(Range.prototype, {
    getClientRects: { configurable: true, value: () => [] },
    getBoundingClientRect: { configurable: true, value: () => new DOMRect() },
  }),
);
afterEach(() => {
  cleanup();
  stop();
});
async function setup() {
  const rpc = new FakeDocumentsDaemon();
  rpc.write(a, "😀a");
  rpc.write(b, "😀b");
  stop = useDocuments.getState().connect(rpc, { journal: memoryJournal() });
  useDocuments.getState().open(a, { kind: "code" });
  await useDocuments.getState().settled();
  return { rpc, uris, current: () => true };
}
test("multi-file edit claims unopened file, uses UTF16 and remains undoable and manually saved", async () => {
  const context = await setup();
  render(<EditorHost docKey={a} />);
  const input = await screen.findByRole("textbox");
  await waitFor(() => expect(input).toHaveAttribute("contenteditable", "true"));
  await act(async () =>
    applyWorkspaceEdit(context, { changes: { [uris.uri(a)]: [edit("A")], [uris.uri(b)]: [edit("B")] } }),
  );
  expect(useDocuments.getState().find(a)?.text).toBe("😀A");
  expect(useDocuments.getState().find(b)?.text).toBe("😀B");
  expect(context.rpc.count("documents.save")).toBe(0);
  act(() => {
    expect(undo(EditorView.findFromDOM(input)!)).toBe(true);
  });
  expect(useDocuments.getState().find(a)?.text).toBe("😀a");
});
test.each(["outside", "resource", "disk", "lease", "generation"])(
  "rejects %s before changing any buffer",
  async (kind) => {
    const context = await setup();
    let changes: unknown = { changes: { [uris.uri(a)]: [edit("A")], [uris.uri(b)]: [edit("B")] } };
    if (kind === "outside")
      changes = { changes: { [uris.uri(a)]: [edit("A")], "file:///elsewhere/b.ts": [edit("B")] } };
    if (kind === "resource")
      changes = {
        documentChanges: [
          { textDocument: { uri: uris.uri(a), version: null }, edits: [edit("A")] },
          { kind: "delete", uri: uris.uri(b) },
        ],
      };
    if (kind === "disk") context.rpc.write(a, "external");
    if (kind === "lease") context.rpc.leases.clear();
    if (kind === "generation") context.current = () => false;
    await expect(applyWorkspaceEdit(context, changes)).rejects.toThrow();
    expect(useDocuments.getState().find(a)?.text).toBe("😀a");
    expect(useDocuments.getState().find(b)?.text ?? "😀b").toBe("😀b");
    expect(context.rpc.count("documents.save")).toBe(0);
  },
);

test.each([
  ["a'b.ts", "a%27b.ts"],
  ["a!b.ts", "a%21b.ts"],
  ["a(b).ts", "a%28b%29.ts"],
  ["a*b.ts", "a%2Ab.ts"],
  ["space %#.ts", "space%20%25%23.ts"],
  ["한😀.ts", "%ED%95%9C%F0%9F%98%80.ts"],
  ["nested/-._~09AZaz.ts", "nested/-._~09AZaz.ts"],
])("R1 matches daemon canonical root and file URI for %s", (path, encoded) => {
  const scoped = projectUris("p", "/owned/'!()* 한😀%#");
  const canonicalRoot = "file:///owned/%27%21%28%29%2A%20%ED%95%9C%F0%9F%98%80%25%23";
  const key = { root: "project:p", path };
  expect(scoped.rootUri).toBe(canonicalRoot);
  expect(scoped.uri(key)).toBe(`${canonicalRoot}/${encoded}`);
  expect(scoped.key(`${canonicalRoot}/${encoded}`)).toEqual(key);
});
test("R1 retains outside-root, encoded separator and malformed UTF8 refusal", () => {
  const scoped = projectUris("p", "/owned/project");
  for (const uri of [
    "file:///outside/a.ts",
    "file:///owned/project/a%2Fb.ts",
    "file:///owned/project/%FF.ts",
    "file:///owned/project/%00.ts",
    "file:///owned/project/%2e%2e/outside.ts",
  ])
    expect(() => scoped.key(uri)).toThrow();
});
