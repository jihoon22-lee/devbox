import { cleanup, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import DistroPanel from "./DistroPanel";

afterEach(() => cleanup());

function baseProps(
  overrides: Partial<ComponentProps<typeof DistroPanel>> = {},
): ComponentProps<typeof DistroPanel> {
  return {
    distros: [],
    selectedDistro: "",
    onSelectDistro: vi.fn(),
    onOpenTerminal: vi.fn(),
    containers: [],
    dockerMissing: false,
    busy: null,
    onAction: vi.fn(),
    onRefresh: vi.fn(),
    snapshotState: "fresh",
    ...overrides,
  };
}

describe("DistroPanel distro state", () => {
  it("exposes explicit fixed-adapter handoff actions without reading or copying output", () => {
    const onOpenJournalInLogLens = vi.fn();
    const onOpenFileInLogLens = vi.fn();
    render(
      <DistroPanel
        {...baseProps({
          distros: [{ name: "Ubuntu", version: 2, default: true, state: "Running" }],
          onOpenJournalInLogLens,
          onOpenFileInLogLens,
        })}
      />,
    );

    screen.getByRole("button", { name: "Log Lens에서 저널 열기" }).click();
    screen.getByRole("button", { name: "Log Lens에서 파일 열기" }).click();
    expect(onOpenJournalInLogLens).toHaveBeenCalledWith("Ubuntu");
    expect(onOpenFileInLogLens).toHaveBeenCalledWith("Ubuntu");
  });

  it("renders a stopped distro with the localized non-running state", () => {
    render(
      <DistroPanel
        {...baseProps({
          distros: [{ name: "Ubuntu 24.04", version: 2, default: false, state: "Stopped" }],
        })}
      />,
    );

    const status = screen.getByText("● 중지됨");
    expect(status).toHaveClass("status-off");
    expect(screen.queryByText("● 실행 중")).toBeNull();
  });

  it("renders a running distro with the running class", () => {
    render(
      <DistroPanel
        {...baseProps({
          distros: [{ name: "Ubuntu", version: 2, default: true, state: "Running" }],
        })}
      />,
    );

    expect(screen.getByText("● 실행 중")).toHaveClass("status-on");
  });

  it("renders the same snapshot's resource and active-terminal summary", () => {
    render(
      <DistroPanel
        {...baseProps({
          distros: [{ name: "Ubuntu", version: 2, default: true, state: "Running" }],
          dashboardDistros: [{
            name: "Ubuntu",
            version: 2,
            default: true,
            state: "Running",
            terminalCount: 3,
            dockerAvailability: "available",
            containers: [],
            resource: {
              cpuPercent: 25,
              memoryUsedBytes: 1024,
              memoryTotalBytes: 2048,
              diskUsedBytes: 3 * 1024,
              diskTotalBytes: 4 * 1024,
            },
          }],
          snapshotState: "fresh",
        })}
      />,
    );

    expect(screen.getByText("3")).toBeInTheDocument();
    expect(screen.getByLabelText("Ubuntu resource summary")).toHaveTextContent("CPU 25%");
    expect(screen.getByRole("status")).toHaveTextContent("최신 snapshot");
  });

  it("marks an old snapshot while allowing the next refresh", () => {
    render(<DistroPanel {...baseProps({ snapshotState: "stale" })} />);
    expect(screen.getByRole("status")).toHaveTextContent("오래된 snapshot");
    expect(screen.getByRole("button", { name: "새로고침" })).toBeEnabled();
  });

  it("shows the shared single-flight state and blocks a second refresh", () => {
    render(<DistroPanel {...baseProps({ snapshotState: "refreshing" })} />);
    expect(screen.getByRole("status")).toHaveTextContent("새로 고치는 중");
    expect(screen.getByRole("button", { name: "새로고침" })).toBeDisabled();
  });

  it("does not start a poll while a Docker action is mutating state", () => {
    render(<DistroPanel {...baseProps({ busy: "abc123:start", snapshotState: "fresh" })} />);
    expect(screen.getByRole("button", { name: "새로고침" })).toBeDisabled();
  });

  it("fails closed for Docker mutations when the shared snapshot is stale", () => {
    render(
      <DistroPanel
        {...baseProps({
          snapshotState: "stale",
          containers: [{ id: "abc123", name: "api", image: "api:latest", status: "Created", ports: "" }],
        })}
      />,
    );
    expect(screen.getByRole("button", { name: "시작" })).toBeDisabled();
  });

  it("does not present a stopped or failed Docker query as an empty list", () => {
    const { rerender } = render(
      <DistroPanel
        {...baseProps({
          distros: [{ name: "Ubuntu", version: 2, default: true, state: "Stopped" }],
          selectedDistro: "Ubuntu",
          dashboardDistros: [{
            name: "Ubuntu",
            version: 2,
            default: true,
            state: "Stopped",
            terminalCount: 0,
            dockerAvailability: "notQueried",
            containers: [],
            resource: null,
          }],
        })}
      />,
    );
    expect(screen.getByText("중지된 WSL 배포판에서는 Docker를 조회하지 않습니다.")).toBeInTheDocument();
    expect(screen.queryByLabelText("Docker 컨테이너")).toBeNull();

    rerender(
      <DistroPanel
        {...baseProps({
          distros: [{ name: "Ubuntu", version: 2, default: true, state: "Running" }],
          selectedDistro: "Ubuntu",
          dashboardDistros: [{
            name: "Ubuntu",
            version: 2,
            default: true,
            state: "Running",
            terminalCount: 0,
            dockerAvailability: "error",
            containers: [],
            resource: null,
          }],
        })}
      />,
    );
    expect(screen.getByText("선택한 WSL 배포판의 Docker 상태를 읽지 못했습니다. 다음 snapshot에서 다시 시도하세요.")).toBeInTheDocument();
    expect(screen.queryByLabelText("Docker 컨테이너")).toBeNull();
  });
});

describe("DistroPanel Docker compact view", () => {
  it("prioritizes name, state, and compact ports in a narrow-panel summary", () => {
    const { container } = render(
      <div style={{ width: 260 }}>
        <DistroPanel
          {...baseProps({
            containers: [
              {
                id: "1234567890abcdef",
                name: "developer-api-with-a-long-container-name",
                image: "registry.example.test/team/api:latest",
                status: "Up 3 hours (healthy)",
                ports:
                  "0.0.0.0:8080->80/tcp, :::8080->80/tcp, 127.0.0.1:9229->9229/tcp, 53/udp",
              },
            ],
          })}
        />
      </div>,
    );

    expect(screen.getByText("Docker (1/1개 실행 중)")).toBeInTheDocument();
    const summary = container.querySelector("summary");
    expect(summary).not.toBeNull();
    expect(summary).toHaveClass("docker-container-summary");
    expect(summary).toHaveTextContent("developer-api-with-a-long-container-name");
    expect(summary).toHaveTextContent("실행 중");
    expect(summary).toHaveTextContent("8080→80/tcp, 9229→9229/tcp +1");
    expect(summary).not.toHaveTextContent("registry.example.test/team/api:latest");
    expect(summary).not.toHaveTextContent("Up 3 hours (healthy)");
    expect(container.querySelector("table")).toBeNull();
  });

  it("keeps exact Docker fields in detail and offers a start action for created containers", () => {
    const onAction = vi.fn();
    render(
      <DistroPanel
        {...baseProps({
          onAction,
          containers: [
            {
              id: "full-container-id",
              name: "worker",
              image: "jobs:sha-123",
              status: "Created",
              ports: "",
            },
          ],
        })}
      />,
    );

    expect(screen.getByText("컨테이너 ID").nextElementSibling).toHaveTextContent(
      "full-container-id",
    );
    expect(screen.getByText("이미지").nextElementSibling).toHaveTextContent("jobs:sha-123");
    expect(screen.getByText("원본 상태").nextElementSibling).toHaveTextContent("Created");
    expect(screen.getByText("원본 포트").nextElementSibling).toHaveTextContent("(비어 있음)");

    screen.getByRole("button", { name: "시작" }).click();
    expect(onAction).toHaveBeenCalledWith("full-container-id", "start");
  });

  it("keeps the install guidance and omits the container list when Docker is missing", () => {
    render(<DistroPanel {...baseProps({ dockerMissing: true })} />);

    expect(screen.getByText(/Docker가 설치되어 있지 않습니다/)).toBeInTheDocument();
    expect(screen.queryByLabelText("Docker 컨테이너")).toBeNull();
  });
});
