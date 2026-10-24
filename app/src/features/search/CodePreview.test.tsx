import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
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
