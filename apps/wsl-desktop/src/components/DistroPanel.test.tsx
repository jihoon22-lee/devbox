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
