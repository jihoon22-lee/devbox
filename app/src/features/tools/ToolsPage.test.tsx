import { act, cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { ApiBackendProvider } from "./api/backend";
import { useApiTabs } from "./api/store";
import { FakeBackend } from "./api/test-backend";
import { toolsSearch } from "./search";
import { ToolsView } from "./ToolsPage";

afterEach(() => {
  cleanup();
  useApiTabs.getState().reset();
  window.innerWidth = 1280;
});
function setup() {
  render(
    <ApiBackendProvider backend={new FakeBackend()}>
      <ToolsView search={{ view: "api" }} projectId={undefined} navigate={() => {}} />
    </ApiBackendProvider>,
  );
}

it("lists_four_tool_groups", async () => {
  setup();
  const list = screen.getByRole("complementary", { name: "목록" });
  const groups = within(list)
    .getAllByRole("heading", { level: 2 })
    .map((h) => h.textContent);
  expect(groups).toEqual(["API 컬렉션", "웹훅", "변환 도구", "개발 환경 점검"]);
  expect(await within(list).findByRole("tree", { name: "개인 컬렉션" })).toBeInTheDocument();
});

it("request_and_response_stack_vertically_below_1100px", () => {
  act(() => {
    useApiTabs.getState().openNew({ kind: "personal" });
  });
  window.innerWidth = 1280;
  setup();
  expect(screen.getByRole("separator", { name: "요청·응답 크기 조절" })).toHaveAttribute(
    "aria-orientation",
    "vertical",
  );
  act(() => {
    window.innerWidth = 1000;
    window.dispatchEvent(new Event("resize"));
  });
  expect(screen.getByRole("separator", { name: "요청·응답 크기 조절" })).toHaveAttribute(
    "aria-orientation",
    "horizontal",
  );
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
