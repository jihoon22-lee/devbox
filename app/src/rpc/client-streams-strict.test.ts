import { expect, test, vi } from "vitest";
import { ClientStreams } from "./client-streams";
import { MAX_STREAM_CHUNK } from "./frame";

test("strict stream writes throw without payload when closed or sending fails", () => {
  const send = vi.fn(() => false);
  const streams = new ClientStreams(send);
  const entry = streams.begin({ onData() {}, onEnd() {} });
  const stream = streams.attach(entry, { streamId: 1 });
  const secret = new TextEncoder().encode("private-lsp-body");
  expect(() => stream.write(secret, { throwOnFailure: true })).toThrow("Stream write failed");
  expect(() => stream.write(secret)).not.toThrow();
  streams.reset();
  expect(() => stream.write(secret, { throwOnFailure: true })).toThrow("Stream write failed");
  expect(() => stream.write(secret)).not.toThrow();
});

test("strict writes detect generation replacement during a partial send", () => {
  let streams: ClientStreams;
  const send = vi.fn(() => {
    streams.reset();
    return true;
  });
  streams = new ClientStreams(send);
  const stream = streams.attach(streams.begin({ onData() {}, onEnd() {} }), { streamId: 1 });
  expect(() => stream.write(new Uint8Array(MAX_STREAM_CHUNK + 1), { throwOnFailure: true })).toThrow(
    "Stream write failed",
  );
  expect(send).toHaveBeenCalledTimes(1);
});
