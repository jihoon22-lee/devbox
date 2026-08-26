import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CookieEditor } from "./CookieEditor";
import type { RequestCookie } from "./types";

afterEach(cleanup);

function setup(
  rows: RequestCookie[],
  secretNames: string[] = [],
  hasRawCookieHeader = false,
) {
  const onChange = vi.fn<(rows: RequestCookie[]) => void>();
  render(
    <CookieEditor
      rows={rows}
      secretNames={secretNames}
      hasRawCookieHeader={hasRawCookieHeader}
      onChange={onChange}
    />,
  );
  return onChange;
}

describe("CookieEditor", () => {
  it("값은 기본적으로 숨기고 사용자가 명시적으로 볼 수 있다", () => {
    setup([{ name: "session", value: "plain-secret", enabled: true }]);
    const value = screen.getByLabelText("1번 cookie 값") as HTMLInputElement;
    expect(value.type).toBe("password");
    fireEvent.click(screen.getByRole("button", { name: "1번 cookie 값 보기" }));
    expect((screen.getByLabelText("1번 cookie 값") as HTMLInputElement).type).toBe("text");
  });

  it("활성 상태·복제·삭제를 순서대로 전달한다", () => {
    const rows = [{ name: "session", value: "abc", enabled: true }];
    const onChange = setup(rows);
    fireEvent.click(screen.getByLabelText("1번 cookie 활성화"));
    expect(onChange).toHaveBeenNthCalledWith(1, [
      { name: "session", value: "abc", enabled: false },
    ]);
    fireEvent.click(screen.getByRole("button", { name: "1번 cookie 복제" }));
    expect(onChange).toHaveBeenNthCalledWith(2, [
      { name: "session", value: "abc", enabled: true },
      { name: "session", value: "abc", enabled: true },
    ]);
    fireEvent.click(screen.getByRole("button", { name: "1번 cookie 삭제" }));
    expect(onChange).toHaveBeenNthCalledWith(3, []);
  });

  it("secret 원문 없이 이름 참조만 삽입한다", () => {
    const onChange = setup(
      [{ name: "session", value: "", enabled: true }],
      ["SESSION", "bad name"],
    );
    const select = screen.getByLabelText("1번 cookie secret 참조") as HTMLSelectElement;
    expect([...select.options].map((option) => option.text)).toEqual(["Secret 참조", "SESSION"]);
    fireEvent.change(select, { target: { value: "SESSION" } });
    expect(onChange).toHaveBeenCalledWith([
      { name: "session", value: "${SESSION}", enabled: true },
    ]);
  });

  it("raw Cookie header 충돌과 잘못된 값의 위치를 표시한다", () => {
    setup([{ name: "session", value: "bad value", enabled: true }], [], true);
    expect(screen.getByRole("alert").textContent).toContain("동시에 전송할 수 없습니다");
    expect(screen.getByText("값에 공백, 세미콜론, 따옴표 또는 제어 문자를 사용할 수 없습니다.")).toBeTruthy();
  });

  it("cookie jar가 아니라 request header 편집임을 항상 알린다", () => {
    const onChange = setup([]);
    expect(screen.getByRole("note").textContent).toContain("현재 요청의 Cookie header");
    fireEvent.click(screen.getByRole("button", { name: "+ Cookie 추가" }));
    expect(onChange).toHaveBeenCalledWith([{ name: "", value: "", enabled: true }]);
  });
});
