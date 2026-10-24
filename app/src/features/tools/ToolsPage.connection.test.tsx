import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { decodeFrame } from "../../rpc/frame";
import { RpcProvider } from "../../rpc/RpcProvider";
import { ProjectWire } from "../projects/test-wire";
import { emptyDraft } from "./api/model/draft";
import { fileFromDraft } from "./api/model/fileDraft";
import { useApiTabs } from "./api/store";
import type { ToolsSearch } from "./search";
import { ToolsPage } from "./ToolsPage";

let route: ToolsSearch = { view: "api", tool: "runner", root: "personal" };

vi.mock("@tanstack/react-router", () => ({
  useSearch: () => route,
  useNavigate: () => vi.fn(),
}));
class DelayedWire extends ProjectWire {
  override send(frame: Uint8Array) {
    const message = JSON.parse(new TextDecoder().decode(decodeFrame(frame).body));
    if (message.type === "hello") {
      this.sent.push(message);
      return;
    }
    super.send(frame);
    if (message.method === "api.collections") {
      queueMicrotask(() =>
        this.deliver({
          type: "response",
          id: message.id,
          result: {
            items: [{ path: "hello.request.json", size: 1, digest: "d1", unreadable: false, reviewed: true }],
            truncated: false,
          },
        }),
      );
    }
    if (message.method === "api.collection_read") {
      queueMicrotask(() =>
        this.deliver({
          type: "response",
          id: message.id,
          result: {
            text: JSON.stringify(fileFromDraft(emptyDraft(), "hello")),
            digest: "d1",
            reviewed: true,
          },
        }),
      );
    }
  }
  welcome() {
    this.deliver({ type: "welcome", protocol: 1, version: "1.0.0-dev", daemonId: "tools" });
  }
}
afterEach(() => {
  cleanup();
  useApiTabs.getState().reset();
  route = { view: "api", tool: "runner", root: "personal" };
});
it("initial runner navigation refreshes failed collection reads after Welcome without executing requests", async () => {
  const wire = new DelayedWire();
  render(
    <RpcProvider client={wire.client}>
      <ToolsPage />
    </RpcProvider>,
  );
  await screen.findByRole("alert");
  expect(wire.requests("api.collections")).toHaveLength(0);
  act(() => wire.welcome());
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  expect(await screen.findByRole("listitem", { name: "1. hello" })).toBeInTheDocument();
  expect(screen.getByRole("treeitem", { name: "hello" })).toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(wire.requests("api.send")).toHaveLength(0);
});

it("a direct request path waits for Welcome and later readiness does not reopen it over another tab", async () => {
  route = { view: "api", root: "personal", path: "hello.request.json" };
  const wire = new DelayedWire();
  render(
    <RpcProvider client={wire.client}>
      <ToolsPage />
    </RpcProvider>,
  );
  await screen.findByRole("alert");
  act(() => wire.welcome());
  expect(await screen.findByRole("textbox", { name: "URL" })).toBeInTheDocument();
  const opened = useApiTabs.getState().tabs[0]!;
  let manual: string;
  act(() => {
    useApiTabs.getState().setDraftById(opened.id, { ...opened.draft, url: "https://edited.invalid/" });
    manual = useApiTabs.getState().openNew({ kind: "personal" });
    wire.close();
    wire.connect();
  });
  await waitFor(() => expect(wire.sent.filter((message) => message.type === "hello")).toHaveLength(2));
  act(() => wire.welcome());
  await waitFor(() => expect(wire.client.state).toBe("ready"));
  expect(useApiTabs.getState().active).toBe(manual!);
  expect(useApiTabs.getState().tabs.find((tab) => tab.id === opened.id)?.draft.url).toBe("https://edited.invalid/");
  expect(wire.requests("api.send")).toHaveLength(0);
});
