import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { RpcError } from "../../rpc/client";
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

test("a file that cannot be read says what to do about it, not only what happened", async () => {
  const wire = new NotesWire();
  vi.spyOn(wire.docs, "call").mockRejectedValue(new RpcError("documents.too_large"));
  render(
    <RpcProvider client={wire.client}>
      <CodePreview
        hit={{ root: "project:owned", path: "big.ts", title: null, snippet: "", line: null, rangesUtf16: [] }}
      />
    </RpcProvider>,
  );
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("문서가 허용 크기를 넘었습니다.");
  expect(alert).toHaveTextContent("내용을 줄인 뒤 다시 시도해 주세요.");
});

test("a file read that failed unexpectedly shows its diagnostic ID", async () => {
  const wire = new NotesWire();
  vi.spyOn(wire.docs, "call").mockRejectedValue(new RpcError("internal", undefined, "diag-3"));
  render(
    <RpcProvider client={wire.client}>
      <CodePreview
        hit={{ root: "project:owned", path: "big.ts", title: null, snippet: "", line: null, rangesUtf16: [] }}
      />
    </RpcProvider>,
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "예상하지 못한 문제가 생겼습니다. 진단 ID를 복사해 두면 원인을 찾을 수 있습니다. · 진단 diag-3",
  );
});
