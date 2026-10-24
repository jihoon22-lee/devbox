import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { RpcProvider } from "../../rpc/RpcProvider";
import { NotesWire } from "../notes/test-wire";
import { CodePreview } from "./CodePreview";

afterEach(cleanup);
test("code results are read through the document boundary and never claim an editing lease", async () => {
  const wire = new NotesWire();
  const key = { root: "project:owned", path: "src/main.ts" };
  wire.docs.write(key, "const owned = true;\n");
  render(
    <RpcProvider client={wire.client}>
      <CodePreview hit={{ ...key, title: null, snippet: "owned", line: 1, rangesUtf16: [] }} />
    </RpcProvider>,
  );
  const editor = await screen.findByRole("textbox", { name: "검색 결과 코드" });
  expect(editor).toHaveAttribute("aria-readonly", "true");
  expect(editor).toHaveTextContent("const owned = true;");
  expect(wire.requests("documents.read")).toEqual([{ key, policy: { kind: "code" } }]);
  expect(wire.requests("documents.claim")).toHaveLength(0);
});
test("edit action carries the exact search file and line without claiming the preview", async () => {
  const wire = new NotesWire(),
    onOpen = vi.fn();
  const hit = { root: "project:owned", path: "src/main.ts", line: 7, title: null, snippet: "owned", rangesUtf16: [] };
  wire.docs.write(hit, "content");
  render(
    <RpcProvider client={wire.client}>
      <CodePreview hit={hit} onOpen={onOpen} />
    </RpcProvider>,
  );
  fireEvent.click(await screen.findByRole("button", { name: "편집기로 열기" }));
  expect(onOpen).toHaveBeenCalledWith(hit);
  expect(wire.requests("documents.claim")).toHaveLength(0);
});
