import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import type { ShellIntegrationReport } from "../types";
import ShellIntegrationSettings from "./ShellIntegrationSettings";

const mocks = vi.hoisted(() => ({
  inspect: vi.fn(),
  update: vi.fn(),
}));

vi.mock("../api", () => ({
  inspectShellIntegration: mocks.inspect,
  updateShellIntegration: mocks.update,
}));

function report(): ShellIntegrationReport {
  return {
    distro: "Ubuntu",
    shells: [
      {
        shell: "bash",
        rcFile: "~/.bashrc",
        status: "missing",
        revision: "bash-r1",
        blockPreview: "# bash block\n",
        defaultShell: true,
      },
      {
        shell: "zsh",
        rcFile: "~/.zshrc",
        status: "current",
        revision: "zsh-r1",
        blockPreview: "# zsh block\n",
        defaultShell: false,
      },
    ],
  };
}

beforeEach(() => {
  mocks.inspect.mockReset().mockImplementation(async () => report());
  mocks.update.mockReset().mockResolvedValue({
    changed: true,
    backupFile: "~/.bashrc.devbox-backup-1-abcd",
    integration: { ...report().shells[0], status: "current", revision: "bash-r2" },
  });
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: vi.fn().mockResolvedValue(undefined) },
  });
});

afterEach(() => cleanup());

describe("ShellIntegrationSettings", () => {
  it("설치 전 marker block을 확인하고 revision이 일치할 때만 변경을 요청한다", async () => {
    const ask = vi.fn().mockResolvedValue({ confirmed: true, value: "", remember: false });
    const onError = vi.fn();
    const { container } = render(
      <ShellIntegrationSettings distro="Ubuntu" ask={ask} onError={onError} />,
    );
    await screen.findByText("미설치");
    await assertNoA11yViolations(container);

    fireEvent.click(screen.getByRole("button", { name: "Bash 연동 설치" }));
    await waitFor(() => expect(ask).toHaveBeenCalledWith(expect.objectContaining({
      title: "Bash 셸 연동을 설치할까요?",
      detail: "# bash block\n",
      danger: true,
    })));
    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith(
      "Ubuntu",
      "bash",
      "install",
      "bash-r1",
    ));
    expect(await screen.findByText(/Bash 연동을 적용했습니다/u)).toHaveTextContent(
      "~/.bashrc.devbox-backup-1-abcd",
    );
    expect(screen.getAllByText("사용 중")).toHaveLength(2);
    expect(onError).not.toHaveBeenCalled();
  });

  it("확인을 취소하면 rc 변경을 요청하지 않는다", async () => {
    const ask = vi.fn().mockResolvedValue({ confirmed: false, value: "", remember: false });
    render(<ShellIntegrationSettings distro="Ubuntu" ask={ask} onError={vi.fn()} />);
    await screen.findByText("미설치");

    fireEvent.click(screen.getByRole("button", { name: "Bash 연동 설치" }));
    await waitFor(() => expect(ask).toHaveBeenCalledTimes(1));
    expect(mocks.update).not.toHaveBeenCalled();
  });

  it("현재 연동 제거와 수동 block 복사를 제공한다", async () => {
    const ask = vi.fn().mockResolvedValue({ confirmed: true, value: "", remember: false });
    render(<ShellIntegrationSettings distro="Ubuntu" ask={ask} onError={vi.fn()} />);
    await screen.findByText("미설치");

    fireEvent.click(screen.getByRole("button", { name: "Zsh block 복사" }));
    await waitFor(() => expect(navigator.clipboard.writeText).toHaveBeenCalledWith("# zsh block\n"));
    fireEvent.click(screen.getByRole("button", { name: "Zsh 연동 제거" }));
    await waitFor(() => expect(ask).toHaveBeenCalledWith(expect.objectContaining({
      title: "Zsh 셸 연동을 제거할까요?",
    })));
  });

  it("충돌 상태에서는 자동 변경을 차단하지만 block 복사는 남긴다", async () => {
    const conflicted = report();
    conflicted.shells[0] = { ...conflicted.shells[0], status: "conflict", revision: "" };
    mocks.inspect.mockResolvedValueOnce(conflicted);
    render(<ShellIntegrationSettings distro="Ubuntu" ask={vi.fn()} onError={vi.fn()} />);

    await screen.findByText("marker 충돌");
    expect(screen.queryByRole("button", { name: "Bash 연동 설치" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Bash block 복사" })).toBeEnabled();
    expect(screen.getByText(/자동 변경하지 않습니다/u)).toBeInTheDocument();
  });

  it("늦게 끝난 이전 배포판 조회 결과를 무시한다", async () => {
    let resolveUbuntu!: (value: ShellIntegrationReport) => void;
    const debian = report();
    debian.distro = "Debian";
    debian.shells[0] = { ...debian.shells[0], status: "current" };
    mocks.inspect.mockImplementationOnce(() => new Promise((resolve) => {
      resolveUbuntu = resolve;
    })).mockResolvedValueOnce(debian);
    const ask = vi.fn();
    const onError = vi.fn();
    const view = render(
      <ShellIntegrationSettings distro="Ubuntu" ask={ask} onError={onError} />,
    );
    view.rerender(<ShellIntegrationSettings distro="Debian" ask={ask} onError={onError} />);
    await waitFor(() => expect(mocks.inspect).toHaveBeenCalledWith("Debian"));
    await waitFor(() => expect(screen.queryByText("미설치")).not.toBeInTheDocument());

    resolveUbuntu(report());
    await Promise.resolve();
    expect(screen.queryByText("미설치")).not.toBeInTheDocument();
  });
});
