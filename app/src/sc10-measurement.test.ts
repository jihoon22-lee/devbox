import { ByteEcho, floodVerdict, MarkerScanner, percentile } from "../acceptance/measurement";

test("producer marker is found before trimming a large callback and across chunk boundaries", () => {
  const scanner = new MarkerScanner("DONE-owned");
  expect(scanner.receive(new TextEncoder().encode("DONE-owned" + "x".repeat(32000)))).toBe(true);
  const split = new MarkerScanner("DONE-owned");
  expect(split.receive(new TextEncoder().encode("x".repeat(32000) + "DONE-"))).toBe(false);
  expect(split.receive(new TextEncoder().encode("owned" + "x".repeat(32000)))).toBe(true);
});

test("echo timing waits for matching one-byte reception and includes frontend send", async () => {
  let clock = 10;
  const echo = new ByteEcho(() => clock);
  const written: number[][] = [];
  const sample = echo.send(
    {
      write(bytes) {
        written.push([...bytes]);
      },
    },
    113,
  );
  echo.receive(Uint8Array.of(122));
  clock = 17;
  echo.receive(Uint8Array.of(27, 91, 72, 113));
  expect(await sample).toBe(7);
  expect(written).toEqual([[113]]);
});

test("missing echo fails and synchronous reception does not lose the sample", async () => {
  vi.useFakeTimers();
  const echo = new ByteEcho(() => 0);
  const absent = echo.send({ write() {} }, 113);
  const rejection = expect(absent).rejects.toThrow("timed out");
  await vi.advanceTimersByTimeAsync(1000);
  await rejection;
  expect(
    await echo.send(
      {
        write(bytes) {
          echo.receive(bytes);
        },
      },
      122,
    ),
  ).toBe(0);
  vi.useRealTimers();
});

test("percentiles use measured samples and flood verdict excludes pings outside output", () => {
  expect(percentile([4, 1, 3, 2], 0.5)).toBe(2);
  expect(percentile([4, 1, 3, 2], 0.95)).toBe(4);
  const during = Array.from({ length: 20 }, () => ({ started: 10, finished: 20, ms: 30 }));
  const outside = [
    { started: 0, finished: 20, ms: 900 },
    { started: 31, finished: 50, ms: 900 },
  ];
  expect(floodVerdict([...during, ...outside], 5, 30, 999)).toMatchObject({ p95: 30, pass: true });
  expect(floodVerdict(during, 5, 30, 1001).pass).toBe(false);
  expect(() => floodVerdict(outside, 5, 30, 1)).toThrow("Insufficient");
});

test("client discovery refuses browser/main and reads an existing native React provider", async () => {
  const { findClient } = await import("../acceptance/terminal-probe");
  expect(() => findClient()).toThrow("native popout");
  vi.stubGlobal("__TAURI_INTERNALS__", { metadata: { currentWindow: { label: "main" } } });
  expect(() => findClient()).toThrow("native popout");
  vi.stubGlobal("__TAURI_INTERNALS__", { metadata: { currentWindow: { label: "popout-owned" } } });
  const element = document.createElement("div");
  const client = { state: "ready", call() {}, openStream() {} };
  Object.assign(element, { __reactFiber$owned: { return: { memoizedProps: { value: { client } } } } });
  document.body.append(element);
  try {
    expect(findClient()).toBe(client);
  } finally {
    element.remove();
    vi.unstubAllGlobals();
  }
});

test("a ping started during output remains in p95 even if it finishes after output", () => {
  const fast = Array.from({ length: 19 }, () => ({ started: 10, finished: 40, ms: 30 }));
  const slow = [
    { started: 10, finished: 160, ms: 150 },
    { started: 10, finished: 1010, ms: 1000 },
  ];
  expect(floodVerdict([...fast, ...slow], 5, 200, 10)).toMatchObject({ p95: 150, pass: false });
});
