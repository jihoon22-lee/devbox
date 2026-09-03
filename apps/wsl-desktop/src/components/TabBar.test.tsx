import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import TabBar from "./TabBar";
import type { Tab } from "../types";

function tab(id: string, title: string): Tab {
  return {
    id,
    title,
    paneIds: [`${id}-pane`],
    layout: "grid",
    customTitle: false,
    sizing: { columns: [1], rows: [1] },
  };
}

function renderBar(overrides: Partial<Parameters<typeof TabBar>[0]> = {}) {
  const props = {
    tabs: [tab("tab-1", "Ubuntu"), tab("tab-2", "Ubuntu 2")],
    activeTabId: "tab-1",
    onActivate: vi.fn(),
    onClose: vi.fn(),
    onRename: vi.fn(),
    onReorder: vi.fn(),
    onDropPane: vi.fn(),
    onNewTab: vi.fn(),
    contextMenuTriggerProps: {},
    actionsDisabled: false,
    ...overrides,
  };
  return { props, ...render(<TabBar {...props} />) };
}

/** 이 RTL 버전에는 fireEvent.auxClick 헬퍼가 없다. */
function auxClick(element: Element, button: number): void {
  fireEvent(element, new MouseEvent("auxclick", { button, bubbles: true, cancelable: true }));
}

afterEach(() => cleanup());

describe("TabBar keyboard selection", () => {
  it("Enter와 Space로 탭을 선택하되 IME와 닫기 어포던스는 선택으로 세지 않는다", () => {
    const onActivate = vi.fn();
    const contextKeyDown = vi.fn();
    const rendered = renderBar({
      tabs: [tab("tab-1", "Ubuntu")],
      activeTabId: "",
      onActivate,
      contextMenuTriggerProps: { onKeyDown: contextKeyDown },
    });
    const pill = rendered.getByLabelText("Ubuntu 터미널 탭");

    fireEvent.keyDown(pill, { key: "Enter", isComposing: true });
    expect(onActivate).not.toHaveBeenCalled();
    fireEvent.keyDown(pill, { key: "Enter" });
    fireEvent.keyDown(pill, { key: " " });
    expect(onActivate).toHaveBeenNthCalledWith(1, "tab-1");
    expect(onActivate).toHaveBeenNthCalledWith(2, "tab-1");
    expect(contextKeyDown).toHaveBeenCalledTimes(3);
  });

  it("좌우·Home·End로 탭 사이를 이동한다", () => {
    const onActivate = vi.fn();
    const rendered = renderBar({ onActivate });
    const pill = rendered.getByLabelText("Ubuntu 터미널 탭");

    fireEvent.keyDown(pill, { key: "ArrowRight" });
    expect(onActivate).toHaveBeenLastCalledWith("tab-2");
    fireEvent.keyDown(pill, { key: "ArrowLeft" });
    expect(onActivate).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(pill, { key: "End" });
    expect(onActivate).toHaveBeenLastCalledWith("tab-2");
    fireEvent.keyDown(rendered.getByLabelText("Ubuntu 2 터미널 탭"), { key: "Home" });
    expect(onActivate).toHaveBeenLastCalledWith("tab-1");
  });

  it("활성 탭만 tab 순서에 남기고 선택 상태를 노출한다", () => {
    const rendered = renderBar();
    const active = rendered.getByLabelText("Ubuntu 터미널 탭");
    const inactive = rendered.getByLabelText("Ubuntu 2 터미널 탭");

    expect(active).toHaveAttribute("aria-selected", "true");
    expect(active).toHaveAttribute("tabindex", "0");
    expect(inactive).toHaveAttribute("aria-selected", "false");
    expect(inactive).toHaveAttribute("tabindex", "-1");
  });

  it("탭 바는 접근성 위반 없이 렌더링된다", async () => {
    const rendered = renderBar();
    await assertNoA11yViolations(rendered.container);
  });
});

describe("TabBar mouse and keyboard closing", () => {
  it("가운데 클릭과 Delete로 탭을 닫는다", () => {
    const onClose = vi.fn();
    const rendered = renderBar({ onClose });
    const pill = rendered.getByLabelText("Ubuntu 터미널 탭");

    auxClick(pill, 1);
    expect(onClose).toHaveBeenLastCalledWith("tab-1");
    fireEvent.keyDown(pill, { key: "Delete" });
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("가운데 클릭이 아닌 보조 버튼은 탭을 닫지 않는다", () => {
    const onClose = vi.fn();
    const rendered = renderBar({ onClose });
    auxClick(rendered.getByLabelText("Ubuntu 터미널 탭"), 2);
    expect(onClose).not.toHaveBeenCalled();
  });

  it("작업이 진행 중이면 닫기 경로가 모두 비활성이다", () => {
    const onClose = vi.fn();
    const rendered = renderBar({ onClose, actionsDisabled: true });
    const pill = rendered.getByLabelText("Ubuntu 터미널 탭");

    auxClick(pill, 1);
    fireEvent.keyDown(pill, { key: "Delete" });
    fireEvent.click(rendered.getAllByTitle("탭 닫기")[0]);
    expect(onClose).not.toHaveBeenCalled();
  });

  it("더블 클릭은 이름 변경을 요청한다", () => {
    const onRename = vi.fn();
    const rendered = renderBar({ onRename });
    fireEvent.doubleClick(rendered.getByLabelText("Ubuntu 터미널 탭"));
    expect(onRename).toHaveBeenCalledWith("tab-1");
  });
});
