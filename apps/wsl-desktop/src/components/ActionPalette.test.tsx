import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ActionPalette from "./ActionPalette";

afterEach(() => cleanup());

describe("ActionPalette", () => {
  it("검색·화살표·Enter로 정확한 action을 실행하고 닫는다", () => {
    const split = vi.fn();
    const search = vi.fn();
    const close = vi.fn();
    render(<ActionPalette open actions={[
      { id: "split", label: "팬: 세로 분할", run: split },
      { id: "search", label: "팬: 출력 검색", run: search },
    ]} onClose={close} />);

    const input = screen.getByRole("textbox", { name: "명령 검색" });
    fireEvent.change(input, { target: { value: "검색" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(search).toHaveBeenCalledTimes(1);
    expect(split).not.toHaveBeenCalled();
    expect(close).toHaveBeenCalledTimes(1);
  });

  it("Escape와 backdrop은 action 없이 닫는다", () => {
    const run = vi.fn();
    const close = vi.fn();
    const { container } = render(<ActionPalette open actions={[
      { id: "close", label: "팬: 닫기", danger: true, run },
    ]} onClose={close} />);
    fireEvent.keyDown(screen.getByRole("textbox", { name: "명령 검색" }), { key: "Escape" });
    fireEvent.mouseDown(container.querySelector(".palette-backdrop") as HTMLDivElement);
    expect(close).toHaveBeenCalledTimes(2);
    expect(run).not.toHaveBeenCalled();
  });
});
