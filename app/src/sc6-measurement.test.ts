import { markSource, ReconnectWatch, rpcMessage } from "../acceptance/sc6-measurement";
import { FRAME, jsonFrame } from "./rpc/frame";

test("source marking replaces exactly one anchor and refuses ambiguous or already-marked files", () => {
  const source = 'const LABEL = { ready: "데몬 연결됨" };\n';
  expect(markSource(source, '"데몬 연결됨"', '"데몬 연결됨 SC6-1"')).toBe(
    'const LABEL = { ready: "데몬 연결됨 SC6-1" };\n',
  );
  expect(() => markSource("", '"데몬 연결됨"', '"x"')).toThrow("exactly once");
  expect(() => markSource(source + source, '"데몬 연결됨"', '"x"')).toThrow("exactly once");
  expect(() => markSource(source, '"데몬 연결됨"', '"데몬 연결됨"')).toThrow("already");
});

test("only JSON frames decode into RPC messages", () => {
  expect(rpcMessage(jsonFrame({ type: "welcome", daemonId: "d1" }))).toEqual({ type: "welcome", daemonId: "d1" });
  expect(rpcMessage(jsonFrame({ bytes: 1 }, FRAME.stream))).toBeUndefined();
  expect(rpcMessage("text")).toBeUndefined();
  expect(rpcMessage(Uint8Array.of(1, 2))).toBeUndefined();
});

test("reconnect completes only after a new daemon answers a data request sent after its welcome", () => {
  const watch = new ReconnectWatch("old");
  watch.sent({ type: "request", id: 1, method: "agents.list" });
  expect(watch.received({ type: "response", id: 1 })).toBe(false);
  expect(watch.received({ type: "welcome", daemonId: "old" })).toBe(false);
  watch.sent({ type: "request", id: 2, method: "agents.list" });
  expect(watch.received({ type: "response", id: 2 })).toBe(false);
  expect(watch.received({ type: "welcome", daemonId: "new" })).toBe(false);
  watch.sent({ type: "request", id: 3, method: "projects.list" });
  watch.sent({ type: "request", id: 4, method: "agents.list" });
  expect(watch.received({ type: "response", id: 3 })).toBe(false);
  expect(watch.received({ type: "response", id: 4, error: { code: "internal" } })).toBe(false);
  watch.sent({ type: "request", id: 5, method: "agents.list" });
  expect(watch.received({ type: "response", id: 5 })).toBe(true);
});
