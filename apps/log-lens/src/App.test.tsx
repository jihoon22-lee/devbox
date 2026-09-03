import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import App from "./App";

const writeText = vi.fn<(value: string) => Promise<void>>();

beforeEach(() => {
  writeText.mockReset().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  });
});

afterEach(() => cleanup());

describe("Log Lens bounded UI", () => {
  it("초기 셸이 접근성 위반 없이 렌더링된다", async () => {
    const { container } = render(<App />);
    await screen.findAllByRole("row");
    await assertNoA11yViolations(container);
  });

  it("offers an accessible log-line context menu and restores row focus", async () => {
    render(<App />);
    const row = screen.getAllByRole("row")[1] as HTMLDivElement;
    row.focus();

    fireEvent.contextMenu(row, { clientX: 18, clientY: 24 });
    expect(screen.getByRole("menu", { name: "로그 줄 작업" })).toBeTruthy();
    fireEvent.click(screen.getByRole("menuitem", { name: "북마크 추가" }));

    await waitFor(() => expect(document.activeElement).toBe(row));
    expect(within(row).getByRole("button", { name: "북마크 제거" })).toBeTruthy();

    fireEvent.keyDown(row, { key: "F10", code: "F10", shiftKey: true });
    fireEvent.click(screen.getByRole("menuitem", { name: "로그 줄 복사" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
    expect(document.activeElement).toBe(row);
  });

  it("shows wrap controls and reports the source cap instead of dropping input", async () => {
    render(<App />);
    const add = screen.getByRole("button", { name: "source 추가" });
    const form = add.closest("form");
    const path = screen.getByPlaceholderText("C:\\logs\\app.log");
    if (!form) throw new Error("source form missing");

    expect(screen.getByLabelText("줄 바꿈")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("줄 바꿈"));
    expect(screen.getAllByRole("row")[1].querySelector(".message.nowrap")).toBeTruthy();

    for (let index = 0; index < 15; index += 1) {
      fireEvent.change(path, { target: { value: `fixture-${index}.log` } });
      fireEvent.submit(form);
      await waitFor(() => expect((add as HTMLButtonElement).disabled).toBe(false));
    }
    expect(screen.getByText(/16\/16개 선택/)).toBeTruthy();

    fireEvent.change(path, { target: { value: "fixture-over-cap.log" } });
    fireEvent.submit(form);
    expect((await screen.findByRole("alert")).textContent).toContain(
      "source는 한 번에 최대 16개까지 불러올 수 있습니다.",
    );
  }, 10_000);

  it("confirms saved-view updates and supports removal", async () => {
    render(<App />);
    fireEvent.change(screen.getByPlaceholderText("뷰 이름"), { target: { value: "Errors" } });
    const save = screen.getByRole("button", { name: "저장" });
    await waitFor(() => expect((save as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(save);
    expect((await screen.findByRole("status")).textContent).toContain("뷰 “Errors”을 저장했습니다.");

    const load = screen.getByLabelText("저장된 뷰 불러오기");
    fireEvent.change(load, { target: { value: "Errors" } });
    expect((await screen.findByRole("status")).textContent).toContain("저장된 뷰 “Errors” 설정을 불러왔습니다. source를 읽으려면 재연결하세요.");
    expect(screen.getByRole("button", { name: "source 재연결" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "source 재연결" }));
    await waitFor(() => expect(screen.getByText("Log Lens 브라우저 미리보기 로그")).toBeTruthy());
    expect(screen.queryByRole("button", { name: "source 재연결" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "뷰 삭제" }));
    expect((await screen.findByRole("status")).textContent).toContain("저장된 뷰 “Errors”을 삭제했습니다.");
    expect(within(load).queryByRole("option", { name: "Errors" })).toBeNull();
  });
});
