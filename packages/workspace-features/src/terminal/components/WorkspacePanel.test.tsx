import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import WorkspacePanel from "./WorkspacePanel";

afterEach(() => cleanup());

describe("WorkspacePanel multiplexer availability", () => {
  it("distinguishes available, missing, and probe errors without exposing a path", () => {
    render(
      <WorkspacePanel
        profiles={[]}
        muxAvailability={[
          { kind: "native", status: "available", version: null, source: null },
          {
            kind: "zellij",
            status: "available",
            version: "zellij 0.41.2",
            source: "userLocal",
          },
          { kind: "tmux", status: "error", version: null, source: null },
        ]}
        busy={false}
        onSaveCurrent={vi.fn()}
        onOpen={vi.fn()}
        onDelete={vi.fn()}
      />,
    );

    const zellij = screen.getByText("zellij: 사용 가능");
    expect(zellij).toHaveClass("available");
    expect(zellij).toHaveAttribute("title", "zellij 0.41.2 · 사용자 로컬");
    expect(zellij).not.toHaveTextContent("/home/");

    const tmux = screen.getByText("tmux: 확인 오류 · native 사용");
    expect(tmux).toHaveClass("error");
    expect(tmux).toHaveAttribute("title", "설치 여부를 확인할 수 없습니다");
  });

  it("shows a missing tool separately from a failed probe", () => {
    render(
      <WorkspacePanel
        profiles={[]}
        muxAvailability={[
          { kind: "native", status: "available", version: null, source: null },
          { kind: "tmux", status: "missing", version: null, source: null },
          { kind: "zellij", status: "missing", version: null, source: null },
        ]}
        busy={false}
        onSaveCurrent={vi.fn()}
        onOpen={vi.fn()}
        onDelete={vi.fn()}
      />,
    );

    expect(screen.getByText("tmux: 없음 · native 사용")).toHaveClass("missing");
    expect(screen.getByText("zellij: 없음 · native 사용")).toHaveClass("missing");
  });
});
