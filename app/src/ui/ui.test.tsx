import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import {
  Banner,
  Button,
  ConfirmDialog,
  Dialog,
  EmptyState,
  ErrorState,
  IconButton,
  Kbd,
  StatusDot,
  TooltipProvider,
} from "./index";

test("buttons support keyboard activation and native disabled behavior", async () => {
  const user = userEvent.setup();
  const click = vi.fn();
  const submit = vi.fn((event: React.FormEvent) => event.preventDefault());
  render(
    <form onSubmit={submit}>
      <Button onClick={click}>저장</Button>
      <Button disabled onClick={click}>
        삭제
      </Button>
    </form>,
  );
  await user.tab();
  await user.keyboard("{Enter} ");
  expect(click).toHaveBeenCalledTimes(2);
  expect(submit).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "삭제" }));
  expect(click).toHaveBeenCalledTimes(2);
});

test("icon buttons expose their label, toggle state and keyboard tooltip", async () => {
  const user = userEvent.setup();
  render(
    <TooltipProvider>
      <IconButton label="새 에이전트 작업" icon={<span>+</span>} shortcut={["Ctrl", "N"]} pressed onClick={() => {}} />
    </TooltipProvider>,
  );
  await user.tab();
  expect(screen.getByRole("button", { name: "새 에이전트 작업" })).toHaveAttribute("aria-pressed", "true");
  expect(await screen.findByRole("tooltip")).toHaveTextContent("Ctrl+N");
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
});

test("disabled icon buttons cannot invoke their action", async () => {
  const user = userEvent.setup();
  const click = vi.fn();
  render(
    <TooltipProvider>
      <IconButton label="삭제" icon={<span>×</span>} disabled onClick={click} />
    </TooltipProvider>,
  );
  const button = screen.getByRole("button", { name: "삭제" });
  expect(button).toBeDisabled();
  await user.click(button);
  expect(click).not.toHaveBeenCalled();
});

test("dialog title and optional description have accessible associations without warnings", () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const error = vi.spyOn(console, "error").mockImplementation(() => {});
  const { rerender } = render(
    <Dialog open onOpenChange={() => {}} title="프로젝트" description="프로젝트를 선택하세요">
      <Button>확인</Button>
    </Dialog>,
  );
  expect(screen.getByRole("dialog", { name: "프로젝트" })).toHaveAccessibleDescription("프로젝트를 선택하세요");
  rerender(
    <Dialog open onOpenChange={() => {}} title="프로젝트">
      <Button>확인</Button>
    </Dialog>,
  );
  expect(screen.getByRole("dialog")).not.toHaveAttribute("aria-describedby");
  expect(warn).not.toHaveBeenCalled();
  expect(error).not.toHaveBeenCalled();
});

test.each(["button", "escape", "backdrop"])(
  "confirm dialog cancels once through %s and restores focus",
  async (dismiss) => {
    const user = userEvent.setup();
    const cancel = vi.fn();
    const confirm = vi.fn();
    function Example() {
      const [open, setOpen] = useState(false);
      return (
        <>
          <Button onClick={() => setOpen(true)}>제거</Button>
          <ConfirmDialog
            open={open}
            title="프로젝트 제거"
            body="목록에서만 지웁니다"
            confirmLabel="프로젝트 제거"
            onConfirm={confirm}
            onCancel={() => {
              cancel();
              setOpen(false);
            }}
          />
        </>
      );
    }
    render(<Example />);
    const trigger = screen.getByRole("button", { name: "제거" });
    await user.click(trigger);
    const cancelButton = screen.getByRole("button", { name: "취소" });
    expect(cancelButton).toHaveFocus();
    await user.tab({ shift: true });
    expect(screen.getByRole("button", { name: "프로젝트 제거" })).toHaveFocus();
    await user.tab();
    expect(cancelButton).toHaveFocus();
    if (dismiss === "button") await user.click(cancelButton);
    if (dismiss === "escape") await user.keyboard("{Escape}");
    if (dismiss === "backdrop") await user.click(document.querySelector("[data-dialog-overlay]") as Element);
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(cancel).toHaveBeenCalledOnce();
    expect(confirm).not.toHaveBeenCalled();
    await waitFor(() => expect(trigger).toHaveFocus());
  },
);

test("confirm invokes only the named action", async () => {
  const user = userEvent.setup();
  const confirm = vi.fn();
  const cancel = vi.fn();
  render(
    <ConfirmDialog
      open
      title="프로젝트 제거"
      body="목록에서만 지웁니다"
      confirmLabel="프로젝트 제거"
      onConfirm={confirm}
      onCancel={cancel}
    />,
  );
  await user.click(screen.getByRole("button", { name: "프로젝트 제거" }));
  expect(confirm).toHaveBeenCalledOnce();
  expect(cancel).not.toHaveBeenCalled();
});

test("diagnostic copy is optional and announces success", async () => {
  const user = userEvent.setup();
  const write = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
  const { rerender } = render(<ErrorState title="저장하지 못했습니다" />);
  expect(screen.queryByRole("button", { name: "진단 복사" })).not.toBeInTheDocument();
  rerender(<ErrorState title="저장하지 못했습니다" diagnosticId="ab12cd34" />);
  await user.click(screen.getByRole("button", { name: "진단 복사" }));
  expect(write).toHaveBeenCalledWith("devbox 진단 ID: ab12cd34");
  expect(screen.getByRole("status")).toHaveTextContent("진단 ID를 복사했습니다");
});

test.each(["rejected", "unavailable"])(
  "diagnostic copy handles %s clipboard without an unhandled rejection",
  async (mode) => {
    const user = userEvent.setup();
    if (mode === "rejected") vi.spyOn(navigator.clipboard, "writeText").mockRejectedValue(new Error("denied"));
    else Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true });
    render(<ErrorState title="저장하지 못했습니다" diagnosticId="ab12cd34" />);
    await user.click(screen.getByRole("button", { name: "진단 복사" }));
    expect(screen.getByRole("status")).toHaveTextContent("복사하지 못했습니다");
    expect(screen.getByText("ab12cd34")).toBeVisible();
  },
);

test("status indicators distinguish every tone by shape and expose names", () => {
  const tones = ["ok", "warn", "danger", "progress", "idle"] as const;
  render(
    <>
      {tones.map((tone) => (
        <StatusDot key={tone} tone={tone} label={tone} />
      ))}
    </>,
  );
  expect(tones.map((tone) => screen.getByRole("img", { name: tone }).getAttribute("data-shape"))).toEqual([
    "circle",
    "triangle",
    "square",
    "diamond",
    "ring",
  ]);
});

test("banners, empty states and shortcut hints preserve content and actions", async () => {
  const user = userEvent.setup();
  const retry = vi.fn();
  render(
    <>
      <Banner tone="danger" actions={<Button onClick={retry}>다시 시도</Button>}>
        연결이 끊겼습니다
      </Banner>
      <EmptyState title="프로젝트가 없습니다" body="프로젝트를 추가하세요" action={<Button>프로젝트 추가</Button>} />
      <Kbd keys={["Ctrl", "K"]} />
    </>,
  );
  expect(screen.getByRole("alert")).toHaveTextContent("연결이 끊겼습니다");
  expect(screen.getByText("프로젝트를 추가하세요")).toBeVisible();
  expect(screen.getByText("Ctrl+K")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "다시 시도" }));
  expect(retry).toHaveBeenCalledOnce();
});
