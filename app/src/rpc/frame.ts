export const FRAME = { json: 0, stream: 1, control: 2 } as const;
export type FrameKind = (typeof FRAME)[keyof typeof FRAME];
export interface Frame {
  kind: FrameKind;
  channel: number;
  body: Uint8Array;
}
const LIMITS = [16 * 1024 * 1024, 65541, 4096] as const;
function validate(kind: number, channel: number, size: number): asserts kind is FrameKind {
  if (!Number.isInteger(kind) || kind < 0 || kind > 2) throw new Error("Unknown frame kind");
  if (!Number.isInteger(channel) || channel < 0 || channel > 0xffffffff) throw new Error("Invalid channel");
  if (size > LIMITS[kind as FrameKind]) throw new Error("Frame exceeds body limit");
}
export function encodeFrame(f: Frame): Uint8Array {
  validate(f.kind, f.channel, f.body.byteLength);
  const out = new Uint8Array(9 + f.body.byteLength);
  const v = new DataView(out.buffer);
  v.setUint32(0, 5 + f.body.byteLength, true);
  v.setUint8(4, f.kind);
  v.setUint32(5, f.channel, true);
  out.set(f.body, 9);
  return out;
}
export function decodeFrame(raw: Uint8Array): Frame {
  if (raw.byteLength < 9) throw new Error("Frame shorter than header");
  const v = new DataView(raw.buffer, raw.byteOffset, raw.byteLength);
  const length = v.getUint32(0, true);
  if (length < 5 || length + 4 !== raw.byteLength) throw new Error("Expected exactly one complete frame");
  const kind = v.getUint8(4),
    channel = v.getUint32(5, true);
  validate(kind, channel, length - 5);
  return { kind, channel, body: raw.slice(9) };
}
export function jsonFrame(msg: unknown, kind: FrameKind = FRAME.json): Uint8Array {
  const json = JSON.stringify(msg);
  if (json === undefined) throw new Error("JSON payload required");
  return encodeFrame({ kind, channel: 0, body: new TextEncoder().encode(json) });
}
