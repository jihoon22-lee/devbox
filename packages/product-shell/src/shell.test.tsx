import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { ProductShell } from "./index";
import { fixtureDescription, isProjectContext, makeRequest, routeStatus } from "./api";
import { navigate, traverse } from "./navigation";
import requestFixture from "../fixtures/route-request.json";
import contextFixture from "../fixtures/project-context.json";

afterEach(cleanup);
describe("product shell", () => {
  it("accepts only bounded opaque native project context and preserves its revision", () => {
    expect(isProjectContext(contextFixture)).toBe(true);
    if (!isProjectContext(contextFixture)) throw new Error("invalid shared fixture");
    const request = makeRequest(fixtureDescription("workspace").handshake, "overview", 1000, contextFixture);
    expect(request.context).toEqual(contextFixture);
    expect(isProjectContext({ ...contextFixture, revision: Number.MAX_SAFE_INTEGER + 1 })).toBe(false);
    expect(isProjectContext({ ...contextFixture, worktreeId: "C:\\unverified-root" })).toBe(false);
    expect(isProjectContext({ ...contextFixture, target: { ...contextFixture.target, path: "/unverified" } })).toBe(false);
  });
  it("shares the native wire fixture and never places a path in route metadata", async () => {
    const description = fixtureDescription("workspace");
    const request = makeRequest(description.handshake, "overview", 1000);
    expect(Object.keys(request).sort()).toEqual(Object.keys(requestFixture).sort());
    expect(request.deadlineMs).toBe(requestFixture.deadlineMs);
    await expect(routeStatus(description, "../../secret")).rejects.toThrow();
    const result = await routeStatus(description, "overview");
    expect(result.provenance.product).toBe("workspace");
  });
  it("bounds navigation and discards forward history when branching", () => {
    let state = { entries: ["overview"], cursor: 0 };
    for (let i = 0; i < 100; i++) state = navigate(state, String(i));
    expect(state.entries.length).toBe(64);
    state = navigate(traverse(state, -2), "files");
    expect(state.entries[state.entries.length - 1]).toBe("files");
    expect(traverse(state, 20).cursor).toBe(state.cursor);
  });
  it("opens all four products with keyboard navigation and accessible empty states", async () => {
    for (const product of ["workspace", "api-studio", "knowledge", "control-center"] as const) {
      const { container, unmount } = render(<ProductShell product={product}/>);
      await screen.findByText("기능 이전을 준비하고 있습니다");
      expect(screen.getByText("브라우저 미리보기 · 모의 데이터")).toBeTruthy();
      const navigation = screen.getByRole("navigation", { name: "제품 화면" });
      const buttons = navigation.querySelectorAll("button");
      fireEvent.click(buttons[1]);
      await waitFor(() => expect(buttons[1].getAttribute("aria-current")).toBe("page"));
      fireEvent.keyDown(navigation, { key: "ArrowLeft", altKey: true, keyCode: 229 });
      expect(buttons[1].getAttribute("aria-current")).toBe("page");
      fireEvent.keyDown(navigation, { key: "ArrowLeft", altKey: true });
      expect(buttons[0].getAttribute("aria-current")).toBe("page");
      await assertNoA11yViolations(container);
      unmount();
    }
  });
});
