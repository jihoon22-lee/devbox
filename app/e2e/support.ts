import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { randomBytes, randomUUID } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createConnection } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { test as base, expect } from "@playwright/test";
import packageJson from "../package.json" with { type: "json" };
import { decodeFrame, jsonFrame } from "../src/rpc/frame";
import type { Methods } from "../src/rpc/gen/rpc";
import { parseServerMessage } from "../src/rpc/server-message";
import { connectFixtureRpc } from "./rpc";
import { stopOwnedTmux } from "./tmux-cleanup";
import { viteAnnounced } from "./vite-readiness";

const { version } = packageJson;

// Actual Claude CLI hook payloads match crates/cli/tests/agents.rs and agents_actions.rs.
const FAKE_AGENT = String.raw`#!/usr/bin/env bash
set -e
session="fake-$DEVBOX_AGENT_ID"
send() { printf '%s' "$1" | "$DEVBOX_BIN" agent event --from claude; }
send "{\"hook_event_name\":\"SessionStart\",\"session_id\":\"$session\"}"
send "{\"hook_event_name\":\"UserPromptSubmit\",\"session_id\":\"$session\"}"
send '{"hook_event_name":"PermissionRequest","tool_name":"Bash"}'
date +%s%3N > "$HOME/agent-hook-$DEVBOX_AGENT_ID.ms"
send '{"hook_event_name":"Notification","notification_type":"permission_prompt","message":"needs Bash"}'
read -r answer
printf 'change\n' > agent.txt
git add agent.txt
git commit -qm 'agent change'
send '{"hook_event_name":"Stop","last_assistant_message":"done"}'
while read -r answer; do send '{"hook_event_name":"Stop","last_assistant_message":"done"}'; done
`;

export async function stop(child: ChildProcess) {
  if (!child.pid || child.exitCode !== null || child.signalCode !== null) return;
  const exited = new Promise<void>((done) => child.once("exit", () => done()));
  child.kill("SIGTERM");
  const timer = setTimeout(() => child.kill("SIGKILL"), 5_000);
  try {
    await exited;
  } finally {
    clearTimeout(timer);
  }
}

export class Stack {
  readonly home = mkdtempSync(join(tmpdir(), "devbox-e2e-"));
  readonly instance = `test-${randomBytes(6).toString("hex")}`;
  readonly token = randomBytes(24).toString("hex");
  readonly gatewayUrl = `ws://127.0.0.1:1461/ws?token=${this.token}`;
  readonly children: ChildProcess[] = [];
  readonly logs = new Map<string, string>();
  readonly env: NodeJS.ProcessEnv;
  daemon?: ChildProcess;
  constructor() {
    this.env = { ...process.env, HOME: this.home, DEVBOX_INSTANCE: this.instance };
    try {
      this.env.DEVBOX_NO_SCOPE = "1";
      this.env.GIT_CONFIG_NOSYSTEM = "1";
      this.env.GIT_CONFIG_GLOBAL = join(this.home, ".gitconfig");
      for (const key of Object.keys(this.env)) {
        if (
          [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_CONFIG",
            "GIT_CONFIG_PARAMETERS",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_SYSTEM",
          ].includes(key) ||
          /^GIT_CONFIG_(KEY|VALUE)_/.test(key)
        )
          delete this.env[key];
      }
      delete this.env.TMUX;
      delete this.env.TMUX_TMPDIR;
      for (const [key, directory] of Object.entries({
        XDG_RUNTIME_DIR: "run",
        XDG_DATA_HOME: "data",
        XDG_STATE_HOME: "state",
        XDG_CONFIG_HOME: "config",
        XDG_CACHE_HOME: "cache",
      })) {
        this.env[key] = join(this.home, directory);
        mkdirSync(this.env[key], { mode: 0o700 });
      }
      mkdirSync(join(this.home, "projects/alpha/.git"), { recursive: true });
      mkdirSync(join(this.home, "projects/beta"), { recursive: true });
      writeFileSync(
        join(this.home, ".gitconfig"),
        "[user]\n name = devbox-test\n email = test@devbox.invalid\n[init]\n defaultBranch = main\n",
      );
      const repo = join(this.home, "projects/gamma");
      mkdirSync(repo, { recursive: true });
      const git = (...args: string[]) =>
        execFileSync("git", ["-C", repo, ...args], { env: this.env, stdio: "pipe", timeout: 5000 });
      git("init", "-b", "main");
      git("config", "user.name", "devbox-test");
      git("config", "user.email", "test@devbox.invalid");
      writeFileSync(join(repo, "initial.txt"), "initial\n");
      git("add", "initial.txt");
      git("commit", "-qm", "initial");
      const config = join(this.env.XDG_CONFIG_HOME as string, "devbox");
      mkdirSync(join(config, this.instance), { recursive: true });
      writeFileSync(
        join(config, this.instance, "tmux.conf"),
        readFileSync(resolve("../crates/terminal/assets/tmux.conf")),
      );
      const tool = join(this.home, "fake-tool.sh");
      writeFileSync(tool, FAKE_AGENT);
      const command = `bash '${tool.replaceAll("'", "'\\''")}'`;
      writeFileSync(
        join(config, "config.toml"),
        `[[agents.profiles]]\nid="fake"\nlabel="Fake"\ncommand=${JSON.stringify(command)}\nhooks="claude"\n`,
      );
    } catch (error) {
      rmSync(this.home, { recursive: true, force: true });
      throw error;
    }
  }
  async withRpc<T>(run: (client: import("../src/rpc/client").RpcClient) => Promise<T>) {
    const client = await connectFixtureRpc(
      join(this.env.XDG_RUNTIME_DIR as string, `devbox-${this.instance}.sock`),
      this.instance,
    );
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      return await Promise.race([
        run(client),
        new Promise<never>((_, reject) => {
          timer = setTimeout(() => {
            client.stop();
            reject(new Error("Fixture RPC operation timed out"));
          }, 10_000);
        }),
      ]);
    } finally {
      clearTimeout(timer);
      client.stop();
    }
  }
  rpc<M extends keyof Methods>(method: M, params: Methods[M]["params"]) {
    return this.withRpc((client) =>
      client.call(method, params, { clientRequestId: randomUUID(), signal: AbortSignal.timeout(10_000) }),
    );
  }
  async gammaProjectId() {
    const path = join(this.home, "projects/gamma");
    const list = await this.rpc("projects.list", {});
    return list.items.find((project) => project.path === path)?.id ?? (await this.rpc("projects.add", { path })).id;
  }
  async waitingAgent(title = "Waiting one") {
    const projectId = await this.gammaProjectId();
    const task = await this.rpc("agents.create", {
      projectId,
      title,
      profile: "fake",
      baseBranch: null,
      worktree: true,
      prompt: "x",
    });
    await expect
      .poll(async () => (await this.rpc("agents.get", { id: task.id })).state, { timeout: 10_000 })
      .toBe("waiting");
    return task.id;
  }
  start(name: string, command: string, args: string[], env = this.env) {
    const child = spawn(command, args, { env, stdio: ["ignore", "pipe", "pipe"] });
    this.children.push(child);
    child.on("error", (error) => this.logs.set(name, `${error}\n${this.logs.get(name) ?? ""}`));
    for (const stream of [child.stdout, child.stderr]) {
      stream?.setEncoding("utf8");
      stream?.on("data", (data) => this.logs.set(name, `${this.logs.get(name) ?? ""}${data}`.slice(-16_384)));
    }
    return child;
  }
  async ready(probe: () => Promise<unknown>) {
    const deadline = Date.now() + 20_000;
    let failure: unknown;
    while (Date.now() < deadline) {
      if (this.children.some((child) => child.exitCode !== null || child.signalCode !== null || !child.pid))
        throw new Error(`Child exited: ${this.diagnostics()}`);
      try {
        await probe();
        return;
      } catch (error) {
        failure = error;
      }
      await delay(100);
    }
    throw new Error(`Readiness timeout: ${failure}\n${this.diagnostics()}`);
  }
  // Real Hello/Welcome readiness; no socket-file existence or fixed startup sleeps.
  hello(): Promise<string> {
    return new Promise((resolveHello, reject) => {
      const socket = createConnection(join(this.env.XDG_RUNTIME_DIR as string, `devbox-${this.instance}.sock`));
      let buffer = Buffer.alloc(0);
      const timer = setTimeout(() => {
        socket.destroy();
        reject(new Error("Hello timed out"));
      }, 1_000);
      socket.on("error", reject);
      socket.on("close", () => {
        clearTimeout(timer);
        reject(new Error("Closed before Welcome"));
      });
      socket.on("connect", () =>
        socket.write(jsonFrame({ type: "hello", version, protocol: 1, client: "test", instance: this.instance })),
      );
      socket.on("data", (data) => {
        buffer = Buffer.concat([buffer, data]);
        if (buffer.length < 4) return;
        const size = buffer.readUInt32LE(0) + 4;
        if (size > 4096) {
          reject(new Error("Oversized Welcome"));
          socket.destroy();
          return;
        }
        if (buffer.length < size) return;
        try {
          const message = parseServerMessage(decodeFrame(buffer.subarray(0, size)).body);
          if (message.type !== "welcome" || message.protocol !== 1 || message.version !== version)
            throw new Error("Expected compatible Welcome");
          resolveHello(message.daemonId);
        } catch (error) {
          reject(error);
        }
        socket.destroy();
      });
    });
  }
  async startDaemon() {
    const binary = process.env.DEVBOX_E2E_BIN;
    if (!binary) throw new Error("global setup did not supply the binary");
    this.daemon = this.start("daemon", binary, ["daemon", "--foreground"]);
    await this.ready(() => this.hello());
  }
  async stopDaemon() {
    if (!this.daemon) throw new Error("No owned daemon");
    await stop(this.daemon);
    this.children.splice(this.children.indexOf(this.daemon), 1);
  }
  async launch() {
    await this.startDaemon();
    const binary = process.env.DEVBOX_E2E_BIN as string;
    this.start("gateway", binary, [
      "dev-gateway",
      "--port",
      "1461",
      "--token",
      this.token,
      "--origin",
      "http://localhost:1460",
    ]);
    // Emitted only after our gateway binds. An occupied port cannot satisfy this probe.
    await this.ready(async () => {
      if (!this.logs.get("gateway")?.includes(`dev-gateway ${this.gatewayUrl}`))
        throw new Error("Gateway has not bound");
    });
    this.start("vite", process.execPath, [resolve("node_modules/vite/bin/vite.js"), "--port", "1460", "--strictPort"], {
      ...this.env,
      VITE_DEVBOX_INSTANCE: this.instance,
      VITE_DEVBOX_WS: this.gatewayUrl,
    });
    await this.ready(async () => {
      if (!viteAnnounced(this.logs.get("vite") ?? "", 1460)) throw new Error("Vite has not bound");
      const response = await fetch("http://localhost:1460", { signal: AbortSignal.timeout(1_000) });
      if (!response.ok) throw new Error(`Vite status ${response.status}`);
    });
  }
  diagnostics() {
    return [...this.logs].map(([name, log]) => `${name}:\n${log}`).join("\n");
  }
  async close() {
    await Promise.all(this.children.map(stop));
    // Daemon restarts preserve sessions; only fixture teardown kills its exact server.
    await stopOwnedTmux(this.instance, this.env);
    rmSync(this.home, { recursive: true, force: true });
  }
}

export const test = base.extend<{ stack: Stack }>({
  stack: async ({}, use, info) => {
    const stack = new Stack();
    try {
      await stack.launch();
      await use(stack);
    } finally {
      try {
        await info.attach("owned-process-logs", { body: stack.diagnostics(), contentType: "text/plain" });
      } finally {
        await stack.close();
      }
    }
  },
});
export { expect };
