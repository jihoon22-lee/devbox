import fixtures from "../../../crates/protocol/fixtures/envelopes.json";
import { decodeFrame, encodeFrame, FRAME, type FrameKind, jsonFrame, parseStreamFrame, streamFrame } from "./frame";

test("shared envelopes round trip and sliced buffers preserve offsets", () => {
  for (const group of Object.values(fixtures))
    for (const msg of Object.values(group)) {
      const raw = jsonFrame(msg);
      const padded = new Uint8Array(raw.length + 8);
      padded.set(raw, 4);
      expect(JSON.parse(new TextDecoder().decode(decodeFrame(padded.subarray(4, -4)).body))).toEqual(msg);
    }
  expect(decodeFrame(encodeFrame({ kind: FRAME.stream, channel: 0xffffffff, body: new Uint8Array([255]) }))).toEqual({
    kind: 1,
    channel: 0xffffffff,
    body: new Uint8Array([255]),
  });
});
test.each([
  [0, 16777216],
  [1, 65541],
  [2, 4096],
])("kind %s enforces Rust body limit", (kind, limit) => {
  const frame = { kind: kind as FrameKind, channel: 0, body: new Uint8Array(limit) };
  expect(decodeFrame(encodeFrame(frame)).body.length).toBe(limit);
  expect(() => encodeFrame({ ...frame, body: new Uint8Array(limit + 1) })).toThrow();
  const raw = new Uint8Array(limit + 10);
  new DataView(raw.buffer).setUint32(0, limit + 6, true);
  raw[4] = kind;
  expect(() => decodeFrame(raw)).toThrow();
});
test("rejects malformed lengths, extra frames, kinds and channels", () => {
  for (const raw of [new Uint8Array(4), new Uint8Array(8), new Uint8Array([...jsonFrame({}), ...jsonFrame({})])])
    expect(() => decodeFrame(raw)).toThrow();
  for (const kind of [-1, 0.5, 3, NaN])
    expect(() => encodeFrame({ kind: kind as FrameKind, channel: 0, body: new Uint8Array() })).toThrow();
  for (const channel of [-1, 0.5, 2 ** 32, NaN])
    expect(() => encodeFrame({ kind: 0, channel, body: new Uint8Array() })).toThrow();
});

test("stream headers round trip sliced input, FIN and u32 IDs", () => {
  const raw = streamFrame(0xffffffff, 1, new Uint8Array([7, 8]));
  const frame = decodeFrame(raw);
  const padded = new Uint8Array(frame.body.length + 8);
  padded.set(frame.body, 4);
  expect(parseStreamFrame(padded.subarray(4, -4))).toEqual({
    streamId: 0xffffffff,
    flags: 1,
    data: new Uint8Array([7, 8]),
  });
  expect(frame.kind).toBe(FRAME.stream);
  expect(frame.channel).toBe(0);
});
test("stream codecs enforce flags, IDs, exact minimum and maximum chunk bytes", () => {
  for (const id of [-1, 1.5, 2 ** 32, NaN]) expect(() => streamFrame(id, 0, new Uint8Array())).toThrow();
  for (const flags of [-1, 2, 1.5, NaN]) expect(() => streamFrame(1, flags, new Uint8Array())).toThrow();
  expect(parseStreamFrame(decodeFrame(streamFrame(1, 0, new Uint8Array(65536))).body).data.length).toBe(65536);
  expect(() => streamFrame(1, 0, new Uint8Array(65537))).toThrow();
  for (const body of [new Uint8Array(4), new Uint8Array(65542), new Uint8Array([1, 0, 0, 0, 2])]) {
    expect(() => parseStreamFrame(body)).toThrow();
  }
});
