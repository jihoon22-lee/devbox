import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import AppDialog, { useAppDialog, type DialogAnswer, type DialogRequest } from "./AppDialog";

function Harness({ request }: { request: DialogRequest }) {
  const { ask, pending, answer } = useAppDialog();
  return (
    <div>
      <button type="button" onClick={() => void ask(request).then((result) => {
        (globalThis as { lastAnswer?: DialogAnswer }).lastAnswer = result;
      })}>열기</button>
      <AppDialog pending={pending} onAnswer={answer} />
    </div>
  );
}

function lastAnswer(): DialogAnswer | undefined {
  return (globalThis as { lastAnswer?: DialogAnswer }).lastAnswer;
}

afterEach(() => {
  cleanup();
  delete (globalThis as { lastAnswer?: DialogAnswer }).lastAnswer;
});

describe("AppDialog", () => {
  it("확인 대화상자는 접근성 위반 없이 렌더링되고 확인 버튼에 focus를 준다", async () => {
    const { container } = render(<Harness request={{ kind: "confirm", title: "팬을 닫을까요?", lines: ["실행 중인 작업이 종료될 수 있습니다."] }} />);
    fireEvent.click(screen.getByRole("button", { name: "열기" }));
    await screen.findByRole("alertdialog");
    await waitFor(() => expect(document.activeElement).toHaveTextContent("확인"));
    await assertNoA11yViolations(container);
  });

  it("Escape는 취소로 resolve하고 opener로 focus를 되돌린다", async () => {
    render(<Harness request={{ kind: "confirm", title: "팬을 닫을까요?" }} />);
    const opener = screen.getByRole("button", { name: "열기" });
    opener.focus();
    fireEvent.click(opener);
    const dialog = await screen.findByRole("alertdialog");

    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() => expect(lastAnswer()).toEqual({ confirmed: false, value: "", remember: false }));
    await waitFor(() => expect(document.activeElement).toBe(opener));
  });

  it("입력 대화상자는 기본값을 채우고 Enter로 값을 확정한다", async () => {
    render(<Harness request={{ kind: "prompt", title: "탭 이름 변경", inputLabel: "탭 이름", defaultValue: "Ubuntu" }} />);
    fireEvent.click(screen.getByRole("button", { name: "열기" }));
    const input = await screen.findByLabelText("탭 이름") as HTMLInputElement;
    expect(input.value).toBe("Ubuntu");

    fireEvent.change(input, { target: { value: "작업 탭" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(lastAnswer()).toEqual({ confirmed: true, value: "작업 탭", remember: false }));
  });

  it("IME 조합 중 Enter는 값을 확정하지 않는다", async () => {
    render(<Harness request={{ kind: "prompt", title: "탭 이름 변경", inputLabel: "탭 이름" }} />);
    fireEvent.click(screen.getByRole("button", { name: "열기" }));
    const input = await screen.findByLabelText("탭 이름");

    fireEvent.keyDown(input, { key: "Enter", keyCode: 229 });
    expect(lastAnswer()).toBeUndefined();
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
  });

  it("기억 체크박스는 확인 결과에 함께 실린다", async () => {
    render(<Harness request={{ kind: "confirm", title: "링크를 열까요?", rememberLabel: "다시 묻지 않기" }} />);
    fireEvent.click(screen.getByRole("button", { name: "열기" }));
    await screen.findByRole("alertdialog");

    fireEvent.click(screen.getByLabelText("다시 묻지 않기"));
    fireEvent.click(screen.getByRole("button", { name: "확인" }));
    await waitFor(() => expect(lastAnswer()).toEqual({ confirmed: true, value: "", remember: true }));
  });

  it("요청이 겹치면 순서대로 하나씩 묻는다", async () => {
    const answers: string[] = [];
    function TwoAsks() {
      const { ask, pending, answer } = useAppDialog();
      return (
        <div>
          <button type="button" onClick={() => {
            void ask({ kind: "confirm", title: "첫 번째" }).then(() => answers.push("첫 번째"));
            void ask({ kind: "confirm", title: "두 번째" }).then(() => answers.push("두 번째"));
          }}>둘 다 열기</button>
          <AppDialog pending={pending} onAnswer={answer} />
        </div>
      );
    }
    render(<TwoAsks />);
    fireEvent.click(screen.getByRole("button", { name: "둘 다 열기" }));

    await screen.findByRole("alertdialog", { name: "첫 번째" });
    expect(screen.queryByRole("alertdialog", { name: "두 번째" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "확인" }));

    await screen.findByRole("alertdialog", { name: "두 번째" });
    fireEvent.click(screen.getByRole("button", { name: "확인" }));
    await waitFor(() => expect(answers).toEqual(["첫 번째", "두 번째"]));
  });

  it("언마운트되면 남은 요청을 취소로 resolve한다", async () => {
    const resolved = vi.fn();
    function Unmounting() {
      const { ask, pending, answer } = useAppDialog();
      return (
        <div>
          <button type="button" onClick={() => void ask({ kind: "confirm", title: "확인" }).then(resolved)}>열기</button>
          <AppDialog pending={pending} onAnswer={answer} />
        </div>
      );
    }
    const { unmount } = render(<Unmounting />);
    fireEvent.click(screen.getByRole("button", { name: "열기" }));
    await screen.findByRole("alertdialog");

    unmount();
    await waitFor(() => expect(resolved).toHaveBeenCalledWith({ confirmed: false, value: "", remember: false }));
  });
});
