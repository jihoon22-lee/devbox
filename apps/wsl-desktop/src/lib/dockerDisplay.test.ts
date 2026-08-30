import { describe, expect, it } from "vitest";
import { compactDockerPorts, dockerDisplayState } from "./dockerDisplay";

describe("dockerDisplayState", () => {
  it.each([
    ["Up 2 hours", "running", true],
    ["Up 5 minutes (Paused)", "paused", false],
    ["Restarting (1) 4 seconds ago", "restarting", false],
    ["Removal In Progress", "removing", false],
    ["Exited (137) 2 minutes ago", "exited", false],
    ["Created", "created", false],
    ["Dead", "dead", false],
    ["", "unknown", false],
  ] as const)("classifies %j as %s", (status, key, running) => {
    expect(dockerDisplayState(status)).toMatchObject({ key, running });
  });
});

describe("compactDockerPorts", () => {
  it("prioritizes published and target ports without host addresses", () => {
    expect(compactDockerPorts("127.0.0.1:8080->80/tcp, 0.0.0.0:5432->5432/tcp")).toBe(
      "8080→80/tcp, 5432→5432/tcp",
    );
  });

  it("deduplicates IPv4 and IPv6 bindings and counts additional mappings", () => {
    expect(
      compactDockerPorts(
        "0.0.0.0:8080->80/tcp, :::8080->80/tcp, 0.0.0.0:8443->443/tcp, 53/udp",
      ),
    ).toBe("8080→80/tcp, 8443→443/tcp +1");
  });

  it("keeps exposed-only ports and represents an empty source", () => {
    expect(compactDockerPorts("6379/tcp")).toBe("6379/tcp");
    expect(compactDockerPorts("  ")).toBe("포트 없음");
  });
});
