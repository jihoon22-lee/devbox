import type { RpcClient } from "../src/rpc/client";
import type { ClientStream } from "../src/rpc/client-streams";
import { ByteEcho, floodVerdict, MarkerScanner, type PingSample, percentile } from "./measurement";

const pause = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));
const encoder = new TextEncoder();
const quote = (s: string) => `'${s.replaceAll("'", "'\\''")}'`;
const owned = new Map<string, { client: RpcClient; stream?: ClientStream }>();
let pingsRunning = false;
let flood:
  | {
      client: RpcClient;
      id: string;
      stream: ClientStream;
      echo: ByteEcho;
      done: Promise<void>;
      started: number;
      finished: number;
    }
  | undefined;

/** Inspect existing React provider props; never attach a second Tauri lease. */
export function findClient(): RpcClient {
  const internal = (
    window as unknown as { __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } } }
  ).__TAURI_INTERNALS__;
  if (!internal?.metadata?.currentWindow?.label?.startsWith("popout-")) {
    throw new Error("Use an owned native popout window; browser/main measurement is not full SC10");
  }
  type Fiber = { return?: Fiber; memoizedProps?: { client?: RpcClient; value?: { client?: RpcClient } } };
  for (const element of document.querySelectorAll("*")) {
    const key = Object.keys(element).find((k) => k.startsWith("__reactFiber$"));
    let fiber = key ? (element as unknown as Record<string, Fiber>)[key] : undefined;
    while (fiber) {
      const candidate = fiber.memoizedProps?.client ?? fiber.memoizedProps?.value?.client;
      if (
        candidate?.state === "ready" &&
        typeof candidate.openStream === "function" &&
        typeof candidate.call === "function"
      ) {
        return candidate;
      }
      fiber = fiber.return;
    }
  }
  throw new Error("No ready production RpcClient in this native window");
}

async function create(client: RpcClient, program: string) {
  const terminal = await client.call("terminal.create", {
    projectId: null,
    cwd: null,
    title: `SC10-${crypto.randomUUID()}`,
    command: `python3 -u -c ${quote(program)}`,
    attachZellij: null,
  });
  owned.set(terminal.id, { client });
  return terminal.id;
}

async function attach(
  client: RpcClient,
  id: string,
  marker: string,
  echo: ByteEcho,
  onBytes?: (bytes: Uint8Array) => void,
) {
  let resolveReady!: () => void;
  let rejectReady!: (error: Error) => void;
  const ready = new Promise<void>((resolve, reject) => {
    resolveReady = resolve;
    rejectReady = reject;
  });
  const scanner = new MarkerScanner(marker);
  const timer = setTimeout(() => rejectReady(new Error("Fixture readiness timed out")), 10000);
  try {
    const stream = await client.openStream(
      "terminal.attach",
      { id, cols: 120, rows: 40, readOnly: false },
      {
        onData(bytes, ack) {
          echo.receive(bytes);
          onBytes?.(bytes);
          if (scanner.receive(bytes)) resolveReady();
          ack();
        },
        onEnd() {
          echo.fail();
          rejectReady(new Error("Fixture stream ended"));
        },
      },
    );
    owned.get(id)!.stream = stream;
    await ready;
    return stream;
  } finally {
    clearTimeout(timer);
  }
}

export async function cleanup(): Promise<void> {
  pingsRunning = false;
  flood = undefined;
  const errors: unknown[] = [];
  for (const [id, resource] of owned) {
    try {
      resource.stream?.end();
      await resource.client.call("terminal.close", { id });
      owned.delete(id);
    } catch (error) {
      errors.push(error);
    }
  }
  if (errors.length) throw new Error(`Could not clean ${errors.length} owned terminals; IDs: ${[...owned.keys()]}`);
}

export async function echo(samples = 200) {
  if (!Number.isInteger(samples) || samples < 20 || samples > 1000) throw new Error("Use 20–1000 samples");
  const client = findClient();
  const marker = `READY${crypto.randomUUID().replaceAll("-", "").toUpperCase()}`;
  // Rewrite one screen cell, so old input cannot match a later repaint sample.
  const program = `import os,tty\ntty.setraw(0)\nos.write(1,b"${marker}")\nwhile True:\n b=os.read(0,1)\n if not b: break\n os.write(1,b"\\r"+b)`;
  const id = await create(client, program);
  const receiver = new ByteEcho();
  try {
    const stream = await attach(client, id, marker, receiver);
    await pause(100); // Drain the attach redraw before the first timed byte.
    const values: number[] = [];
    for (let i = 0; i < samples + 10; i++) {
      const ms = await receiver.send(stream, [113, 122, 119, 98][i % 4]!);
      if (i >= 10) values.push(ms); // Ten disclosed warm-up samples.
      await pause(10);
    }
    const p50 = percentile(values, 0.5);
    const p95 = percentile(values, 0.95);
    return {
      path: "frontend→Tauri→Windows wsl.exe bridge→daemon PTY→tmux→return",
      excludes: "screen drawing",
      inputBytes: 1,
      warmup: 10,
      samples: values,
      p50,
      p95,
      pass: p50 <= 10 && p95 <= 25,
    };
  } finally {
    receiver.fail();
    owned.get(id)?.stream?.end();
    await client.call("terminal.close", { id });
    owned.delete(id);
  }
}

/** Arm output, start pingUntilStopped on a DIFFERENT native popout, then startFlood. */
export async function beginFlood() {
  if (flood) throw new Error("Flood already active");
  const client = findClient();
  const token = crypto.randomUUID().replaceAll("-", "").toUpperCase();
  const ready = `READY${token}`;
  const doneMarker = `DONE${token}`;
  // Exact 100,000,000 producer bytes, without producer pacing.
  const program = `import os,tty\ntty.setraw(0)\nos.write(1,b"${ready}")\nos.read(0,1)\nleft=100000000\nwhile left:\n n=min(left,65536)\n data=(b"x"*79+b"\\n")*820\n offset=0\n while offset<n: offset+=os.write(1,data[offset:n])\n left-=n\nos.write(1,b"\\r\\n${doneMarker}\\r\\n")\nwhile True:\n b=os.read(0,1)\n if not b: break\n os.write(1,b"\\r"+b)`;
  const id = await create(client, program);
  const receiver = new ByteEcho();
  let resolveDone!: () => void;
  const done = new Promise<void>((resolve) => {
    resolveDone = resolve;
  });
  const scanner = new MarkerScanner(doneMarker);
  let finished = 0;
  try {
    const stream = await attach(client, id, ready, receiver, (bytes) => {
      if (!finished && scanner.receive(bytes)) {
        finished = Date.now();
        if (flood) flood.finished = finished;
        resolveDone();
      }
    });
    flood = { client, id, stream, echo: receiver, done, started: 0, finished };
    return { terminalId: id, bytes: 100000000 };
  } catch (error) {
    await cleanup();
    throw error;
  }
}
export function startFlood() {
  if (!flood || flood.started) throw new Error("Flood not armed or already started");
  flood.started = Date.now();
  flood.stream.write(encoder.encode("!"));
  return flood.started;
}
export async function finishFlood() {
  const active = flood;
  if (!active?.started) throw new Error("Flood not started");
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    await Promise.race([
      active.done,
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new Error("100MB output did not finish within 60s")), 60000);
      }),
    ]);
    const recoveryMs = await active.echo.send(active.stream, 113, 1000);
    return { started: active.started, finished: active.finished, bytes: 100000000, recoveryMs };
  } finally {
    clearTimeout(timer);
    await cleanup();
  }
}
export async function pingUntilStopped() {
  if (pingsRunning) throw new Error("Ping measurement already running");
  const client = findClient();
  pingsRunning = true;
  const samples: PingSample[] = [];
  const deadline = Date.now() + 65000;
  while (pingsRunning && Date.now() < deadline) {
    const started = Date.now();
    const t0 = performance.now();
    await client.call("system.ping", {});
    samples.push({ started, finished: Date.now(), ms: performance.now() - t0 });
  }
  pingsRunning = false;
  return samples;
}
export function stopPings() {
  pingsRunning = false;
}
export { floodVerdict };
