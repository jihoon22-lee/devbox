import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import TabBar from "./TabBar";

afterEach(() => cleanup());

describe("TabBar keyboard selection", () => {
  it("Enter와 Space로 탭을 선택하되 IME와 닫기 버튼 이벤트는 무시한다", () => {
    const onActivate = vi.fn();
    const contextKeyDown = vi.fn();
    const rendered = render(
      <TabBar
        tabs={[{ id: "tab-1", title: "Ubuntu", paneIds: [], layout: "grid", customTitle: false }]}
        activeTabId=""
        onActivate={onActivate}
        onClose={vi.fn()}
        onReorder={vi.fn()}
        onDropPane={vi.fn()}
        onNewTab={vi.fn()}
        contextMenuTriggerProps={{ onKeyDown: contextKeyDown }}
        actionsDisabled={false}
      />,
    );
    const tab = rendered.getByLabelText("Ubuntu 터미널 탭");

    fireEvent.keyDown(tab, { key: "Enter", isComposing: true });
    expect(onActivate).not.toHaveBeenCalled();
    fireEvent.keyDown(tab, { key: "Enter" });
    fireEvent.keyDown(tab, { key: " " });
    expect(onActivate).toHaveBeenNthCalledWith(1, "tab-1");
    expect(onActivate).toHaveBeenNthCalledWith(2, "tab-1");

    fireEvent.keyDown(rendered.getByTitle("탭 닫기"), { key: "Enter" });
    expect(onActivate).toHaveBeenCalledTimes(2);
    expect(contextKeyDown).toHaveBeenCalledTimes(4);
  });
});
