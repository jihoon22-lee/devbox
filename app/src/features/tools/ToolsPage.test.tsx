import { act, cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { ApiBackendProvider } from "./api/backend";
import { emptyDraft, setBodyKind } from "./api/model/draft";
import { useApiTabs } from "./api/store";
import { FakeBackend } from "./api/test-backend";
import { type ToolsSearch, toolsSearch } from "./search";
import { ToolsView } from "./ToolsPage";

afterEach(() => {
  cleanup();
  useApiTabs.getState().reset();
  window.innerWidth = 1280;
});
function setup(search: ToolsSearch = { view: "api" }) {
  render(
    <ApiBackendProvider backend={new FakeBackend()}>
      <ToolsView search={search} projectId={undefined} navigate={() => {}} />
    </ApiBackendProvider>,
  );
}

it("tools_list_has_four_groups", async () => {
  setup();
  const list = screen.getByRole("complementary", { name: "목록" });
  const groups = within(list)
    .getAllByRole("heading", { level: 2 })
    .map((h) => h.textContent);
  expect(groups).toEqual(["API 컬렉션", "웹훅", "변환 도구", "개발 환경 점검"]);
  expect(await within(list).findByRole("tree", { name: "개인 컬렉션" })).toBeInTheDocument();
});

it("narrow_width_stacks_response_below", () => {
  act(() => {
    useApiTabs.getState().openNew({ kind: "personal" });
  });
  window.innerWidth = 1100;
  setup();
  expect(screen.getByRole("separator", { name: "요청·응답 크기 조절" })).toHaveAttribute(
    "aria-orientation",
    "vertical",
  );
  act(() => {
    window.innerWidth = 1099;
    window.dispatchEvent(new Event("resize"));
  });
  expect(screen.getByRole("separator", { name: "요청·응답 크기 조절" })).toHaveAttribute(
    "aria-orientation",
    "horizontal",
  );
  expect(screen.getByRole("separator", { name: "요청·응답 크기 조절" }).parentElement).toHaveAttribute(
    "data-dir",
    "column",
  );
  act(() => {
    window.innerWidth = 1100;
    window.dispatchEvent(new Event("resize"));
  });
  expect(screen.getByRole("separator", { name: "요청·응답 크기 조절" }).parentElement).toHaveAttribute(
    "data-dir",
    "row",
  );
});

it.each([
  ["HTTP", undefined, "쿼리 추가"],
  ["GraphQL", undefined, "GraphQL 쿼리"],
  ["WebSocket", "websocket", "하위 프로토콜"],
  ["SSE", "sse", "Last-Event-ID"],
  ["gRPC", "grpc", "gRPC 대상"],
  ["MCP", "mcp", "연결 방식"],
] as const)("request_kind_shows_only_its_panel: %s", async (kind, tool, field) => {
  const tabs = useApiTabs.getState();
  const key = tabs.openNew({ kind: "personal" });
  tabs.update(key, { draft: kind === "GraphQL" ? setBodyKind(emptyDraft(), "graphql") : emptyDraft() });
  setup({ view: "api", tool });
  if (kind === "HTTP") expect(await screen.findByRole("button", { name: field })).toBeInTheDocument();
  else expect(await screen.findByLabelText(field)).toBeInTheDocument();
  for (const label of ["GraphQL 쿼리", "하위 프로토콜", "Last-Event-ID", "gRPC 대상", "연결 방식"]) {
    if (label !== field) expect(screen.queryByLabelText(label)).not.toBeInTheDocument();
  }
  if (kind !== "HTTP") expect(screen.queryByRole("button", { name: "쿼리 추가" })).not.toBeInTheDocument();
  for (const name of ["WebSocket", "SSE", "gRPC", "MCP"]) {
    expect(screen.queryByRole("region", { name })).toBe(kind === name ? screen.getByRole("region", { name }) : null);
  }
  expect(screen.queryByRole("tablist", { name: "요청 설정" }) !== null).toBe(!tool);
});

it("grpc_and_mcp_tools_open_from_the_list_and_search", async () => {
  const seen: unknown[] = [];
  const { rerender } = render(
    <ApiBackendProvider backend={new FakeBackend()}>
      <ToolsView search={{ view: "api" }} projectId={undefined} navigate={(next) => seen.push(next)} />
    </ApiBackendProvider>,
  );
  const list = screen.getByRole("complementary", { name: "목록" });
  within(list).getByRole("button", { name: "gRPC" }).click();
  within(list).getByRole("button", { name: "MCP" }).click();
  expect(seen).toEqual([
    { view: "api", tool: "grpc" },
    { view: "api", tool: "mcp" },
  ]);
  expect(toolsSearch({ view: "api", tool: "grpc" })).toEqual({ view: "api", tool: "grpc" });
  expect(toolsSearch({ view: "api", tool: "mcp" })).toEqual({ view: "api", tool: "mcp" });
  for (const [tool, label] of [
    ["grpc", "gRPC 대상"],
    ["mcp", "연결 방식"],
  ] as const) {
    rerender(
      <ApiBackendProvider backend={new FakeBackend()}>
        <ToolsView search={{ view: "api", tool }} projectId={undefined} navigate={() => {}} />
      </ApiBackendProvider>,
    );
    expect(await screen.findByLabelText(label)).toBeInTheDocument();
  }
});
