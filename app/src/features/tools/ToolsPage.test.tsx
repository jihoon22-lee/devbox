import { act, cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { ApiBackendProvider } from "./api/backend";
import { useApiTabs } from "./api/store";
import { FakeBackend } from "./api/test-backend";
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
