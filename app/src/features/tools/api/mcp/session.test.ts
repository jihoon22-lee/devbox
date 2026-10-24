import { expect, it } from "vitest";
import {
  applyMcp,
  commandLine,
  effectiveCwd,
  envPairs,
  initialMcp,
  type McpState,
  parseMcpFile,
  toTransport,
} from "./session";

const stdioFile = JSON.stringify({
  version: 1,
  kind: "mcp",
  name: "srv",
  transport: {
    kind: "stdio",
    command: "python3",
    args: ["/p/server.py", "a b", "it's"],
    cwd: "/p",
    env: [
      { name: "TOKEN", value: "{{secret:TOKEN}}" },
      { name: "OFF", value: "x", enabled: false },
    ],
  },
});

it("mcp_file_reads_stdio_and_http_definitions", () => {
  const stdio = parseMcpFile(stdioFile);
  expect(stdio.name).toBe("srv");
  expect(toTransport(stdio.transport)).toEqual({
    kind: "stdio",
    command: "python3",
    args: ["/p/server.py", "a b", "it's"],
    cwd: "/p",
    env: [{ name: "TOKEN", value: "{{secret:TOKEN}}" }],
  });
  const http = parseMcpFile(
    JSON.stringify({
      version: 1,
      kind: "mcp",
      name: "remote",
      transport: { kind: "http", url: "https://mcp.test/mcp", headers: [{ name: "X-K", value: "v" }] },
    }),
  );
  expect(toTransport(http.transport)).toEqual({
    kind: "http",
    url: "https://mcp.test/mcp",
    headers: [{ row: 0, name: "X-K", value: "v" }],
    tls: { verify: true, caPath: null, clientCertPath: null, clientKeyPath: null },
  });
  for (const bad of ["{", '{"version":1,"kind":"grpc"}', '{"version":1,"kind":"mcp","transport":{"kind":"stdio"}}'])
    expect(() => parseMcpFile(bad)).toThrow(/MCP 파일/);
});

it("command_line_env_values_and_cwd_are_shown_exactly", () => {
  const { transport } = parseMcpFile(stdioFile);
  expect(commandLine(transport)).toBe(`python3 /p/server.py 'a b' 'it'\\''s'`);
  expect(commandLine({ ...transport, kind: "stdio", command: "", args: [""] } as never)).toBe(`'' ''`);
  // 값은 적힌 그대로(비밀 참조는 풀지 않은 참조로), 꺼진 행은 보내지 않으니 보이지 않는다.
  expect(envPairs(transport)).toEqual(["TOKEN={{secret:TOKEN}}"]);
  expect(effectiveCwd(transport, null)).toBe("/p");
  const noCwd = { ...transport, cwd: null } as typeof transport;
  expect(effectiveCwd(noCwd, { label: "devbox" })).toBe("프로젝트 루트(devbox)");
  expect(effectiveCwd(noCwd, null)).toBeNull();
});

it("mcp_state_follows_ready_lists_and_results", () => {
  let state: McpState = initialMcp();
  expect(state.status).toBe("connecting");
  state = applyMcp(state, { type: "authorize", url: "https://as.test/authorize?x" });
  expect(state.authorizeUrl).toBe("https://as.test/authorize?x");
  state = applyMcp(state, {
    type: "ready",
    serverName: "fixture",
    serverVersion: "1",
    protocolVersion: "2025-06-18",
    capabilitiesJson: "{}",
  });
  expect(state.status).toBe("ready");
  expect(state.authorizeUrl).toBeNull();
  state = { ...state, pending: { l1: { kind: "list", what: "tools" }, c1: { kind: "call", label: "echo" } } };
  state = applyMcp(state, {
    type: "result",
    id: "l1",
    json: JSON.stringify({ tools: [{ name: "echo", description: "Echo" }, { name: 3 }] }),
  });
  expect(state.tools.map((t) => t.name)).toEqual(["echo"]);
  expect(state.pending.l1).toBeUndefined();
  state = applyMcp(state, { type: "error", id: "c1", code: -32602, message: "bad" });
  expect(state.results[0]).toMatchObject({ id: "c1", label: "echo", error: "-32602 bad" });
  state = applyMcp(state, { type: "failed", error: { code: "api.mcp", detail: { reason: "spawn" } } });
  expect(state.status).toBe("failed");
  expect(applyMcp(state, { type: "closed" }).status).toBe("failed");
  expect(applyMcp(initialMcp(), { type: "closed" }).status).toBe("closed");
});
