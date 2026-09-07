import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { HeaderTable } from "./HeaderTable";
import type { RequestHeader } from "./types";

afterEach(cleanup);

function setup(rows: RequestHeader[], secretNames: string[] = []) {
  const onChange = vi.fn<(rows: RequestHeader[]) => void>();
  render(<HeaderTable rows={rows} secretNames={secretNames} onChange={onChange} />);
  return onChange;
}

describe("HeaderTable", () => {
  it("중복 header와 enabled 상태를 표시하고 toggle해도 순서를 보존한다", () => {
    const rows = [
      { key: "X-Trace", value: "one", enabled: true },
      { key: "x-trace", value: "two", enabled: false },
    ];
    const onChange = setup(rows);

    expect(screen.getByText("활성 1 / 전체 2 · 중복 이름 1개")).toBeTruthy();
    expect((screen.getByLabelText("1번 header 활성화") as HTMLInputElement).checked).toBe(true);
    expect((screen.getByLabelText("2번 header 활성화") as HTMLInputElement).checked).toBe(false);

    fireEvent.click(screen.getByLabelText("2번 header 활성화"));
    expect(onChange).toHaveBeenCalledWith([
      { key: "X-Trace", value: "one", enabled: true },
      { key: "x-trace", value: "two", enabled: true },
    ]);
  });

  it("행 복제와 삭제가 duplicate value/enabled를 정확히 보존한다", () => {
    const rows = [{ key: "X-Trace", value: "one", enabled: false }];
    const onChange = setup(rows);

    fireEvent.click(screen.getByRole("button", { name: "1번 header 복제" }));
    expect(onChange).toHaveBeenNthCalledWith(1, [
      { key: "X-Trace", value: "one", enabled: false },
      { key: "X-Trace", value: "one", enabled: false },
    ]);

    fireEvent.click(screen.getByRole("button", { name: "1번 header 삭제" }));
    expect(onChange).toHaveBeenNthCalledWith(2, []);
  });

  it("현재 환경의 secret 이름만 reference로 삽입하고 원문 값 prop을 요구하지 않는다", () => {
    const onChange = setup(
      [{ key: "Authorization", value: "", enabled: true }],
      ["TOKEN", "bad name", "API_KEY"],
    );

    const select = screen.getByLabelText("1번 header secret 참조") as HTMLSelectElement;
    expect([...select.options].map((option) => option.text)).toEqual([
      "Secret 참조",
      "API_KEY",
      "TOKEN",
    ]);
    fireEvent.change(select, { target: { value: "TOKEN" } });
    expect(onChange).toHaveBeenCalledWith([
      { key: "Authorization", value: "${TOKEN}", enabled: true },
    ]);
    expect(screen.getByRole("note").textContent).toContain("봉인된 원문을 읽거나 표시하지 않습니다");
  });

  it("secret이 없어도 enabled 기본 행을 추가할 수 있다", () => {
    const onChange = setup([]);
    expect((screen.getByLabelText("요청 Header 편집").querySelector("select") as HTMLSelectElement | null)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "+ 헤더 추가" }));
    expect(onChange).toHaveBeenCalledWith([{ key: "", value: "", enabled: true }]);
  });
});
