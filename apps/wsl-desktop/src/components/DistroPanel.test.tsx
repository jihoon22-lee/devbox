import { cleanup, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import DistroPanel from "./DistroPanel";

afterEach(() => cleanup());

function baseProps(overrides: Partial<ComponentProps<typeof DistroPanel>> = {}) {
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
    ...overrides,
  };
}

describe("DistroPanel distro state", () => {
  it("renders a stopped distro as Stopped with the non-running class", () => {
    render(
      <DistroPanel
        {...baseProps({
          distros: [{ name: "Ubuntu 24.04", version: 2, default: false, state: "Stopped" }],
        })}
      />,
    );

    const status = screen.getByText("● Stopped");
    expect(status).toHaveClass("status-off");
    expect(screen.queryByText("● Running")).toBeNull();
  });

  it("renders a running distro with the running class", () => {
    render(
      <DistroPanel
        {...baseProps({
          distros: [{ name: "Ubuntu", version: 2, default: true, state: "Running" }],
        })}
      />,
    );

    expect(screen.getByText("● Running")).toHaveClass("status-on");
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

    expect(screen.getByText("Docker (1/1 running)")).toBeInTheDocument();
    const summary = container.querySelector("summary");
    expect(summary).not.toBeNull();
    expect(summary).toHaveClass("docker-container-summary");
    expect(summary).toHaveTextContent("developer-api-with-a-long-container-name");
    expect(summary).toHaveTextContent("Running");
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

    expect(screen.getByText("Container ID").nextElementSibling).toHaveTextContent(
      "full-container-id",
    );
    expect(screen.getByText("Image").nextElementSibling).toHaveTextContent("jobs:sha-123");
    expect(screen.getByText("Original status").nextElementSibling).toHaveTextContent("Created");
    expect(screen.getByText("Original ports").nextElementSibling).toHaveTextContent("(empty)");

    screen.getByRole("button", { name: "Start" }).click();
    expect(onAction).toHaveBeenCalledWith("full-container-id", "start");
  });

  it("keeps the install guidance and omits the container list when Docker is missing", () => {
    render(<DistroPanel {...baseProps({ dockerMissing: true })} />);

    expect(screen.getByText(/Docker가 설치되어 있지 않습니다/)).toBeInTheDocument();
    expect(screen.queryByLabelText("Docker containers")).toBeNull();
  });
});
