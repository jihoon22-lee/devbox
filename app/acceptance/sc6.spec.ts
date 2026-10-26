import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, utimesSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "@playwright/test";
import { markSource, ReconnectWatch, rpcMessage } from "./sc6-measurement";

// SC6 raw samples on this harness's monotonic clock only; dev-session's own stamps are separate evidence.
const root = fileURLToPath(new URL("../..", import.meta.url));
const statusBar = join(root, "app/src/shell/StatusBar.tsx");
const daemonSource = join(root, "crates/cli/src/daemon/mod.rs");
const anchor = '"데몬 연결됨"';
const count = Number(process.env.DEVBOX_SC6_SAMPLES ?? 5);
const sha = (bytes: Buffer) => createHash("sha256").update(bytes).digest("hex");
const painted = () =>
  new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));

function ownedSession(): number {
  const pid = Number(process.env.DEVBOX_SC6_SESSION_PID);
  // Refuse anything but the operator's own scripts/dev-session.py process.
  const command = Number.isInteger(pid) && pid > 1 ? readFileSync(`/proc/${pid}/cmdline`, "utf8") : "";
  if (!command.split("\0").some((part) => part.endsWith("scripts/dev-session.py")))
    throw new Error("Set DEVBOX_SC6_SESSION_PID to the owned scripts/dev-session.py process");
  return pid;
}

test("SC6 development loop raw timings", async ({ page }) => {
  test.setTimeout(30 * 60_000);
  const session = ownedSession();
  let daemonId: string | undefined;
  let watch: ReconnectWatch | undefined;
  let reconnected: (() => void) | undefined;
  page.on("websocket", (socket) => {
    socket.on("framesent", ({ payload }) => {
      const message = rpcMessage(payload);
      if (message) watch?.sent(message);
    });
    socket.on("framereceived", ({ payload }) => {
      const message = rpcMessage(payload);
      if (message?.type === "welcome") daemonId = message.daemonId;
      if (message && watch?.received(message)) reconnected?.();
    });
  });
  await page.goto(process.env.DEVBOX_SC6_URL ?? "http://localhost:1450/");
  await expect(page.getByText("데몬 연결됨").first()).toBeVisible({ timeout: 30_000 });
  const ui: number[] = [];
  const original = readFileSync(statusBar);
  execFileSync("git", ["diff", "--quiet", "--", statusBar], { cwd: root });
  try {
    for (let index = 0; index < count; index++) {
      const marker = `SC6-${process.pid}-${index}`;
      const next = markSource(original.toString("utf8"), anchor, `"데몬 연결됨 ${marker}"`);
      const started = performance.now();
      writeFileSync(statusBar, next);
      await page.waitForFunction((text) => document.body.innerText.includes(text), marker, { polling: "raf" });
      ui.push(performance.now() - started);
      writeFileSync(statusBar, original);
      await page.waitForFunction((text) => !document.body.innerText.includes(text), marker, { polling: "raf" });
    }
  } finally {
    writeFileSync(statusBar, original);
    expect(sha(readFileSync(statusBar))).toBe(sha(original));
  }
  const daemon: { buildMs: number; restartMs: number; totalMs: number }[] = [];
  for (let index = 0; index < count; index++) {
    // Content-preserving owned change: a new mtime forces cargo to recompile the daemon crate.
    const now = new Date();
    utimesSync(daemonSource, now, now);
    const buildStarted = performance.now();
    const build = spawnSync("cargo", ["build", "--locked", "--bin", "devbox"], { cwd: root, stdio: "inherit" });
    const buildMs = performance.now() - buildStarted;
    if (build.status !== 0) throw new Error(`cargo build failed with ${build.status}`);
    const done = new Promise<void>((resolve) => {
      reconnected = resolve;
    });
    watch = new ReconnectWatch(daemonId);
    const started = performance.now();
    process.kill(session, "SIGUSR1");
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([
        done,
        new Promise<never>((_, reject) => {
          timer = setTimeout(() => reject(new Error("SC6 reconnect not observed within 60s")), 60_000);
        }),
      ]);
    } finally {
      clearTimeout(timer);
    }
    await expect(page.getByText("데몬 연결됨").first()).toBeVisible();
    await expect(page.getByText(/다시 연결하고 있습니다/)).toHaveCount(0);
    await expect(page.getByRole("button", { name: /^에이전트 \d+/ })).toBeVisible();
    await page.evaluate(painted);
    const restartMs = performance.now() - started;
    watch = undefined;
    daemon.push({ buildMs, restartMs, totalMs: buildMs + restartMs });
  }
  const result = { criterion: "SC6", clock: "harness performance.now()", ui, daemon };
  console.log(JSON.stringify(result));
  if (process.env.DEVBOX_SC6_OUTPUT)
    writeFileSync(process.env.DEVBOX_SC6_OUTPUT, `${JSON.stringify(result, null, 2)}\n`);
});
