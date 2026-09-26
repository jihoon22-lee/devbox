export interface OutputBatch {
  frames: { sequence: number; data: string }[];
  cursor: number;
  truncated: boolean;
  closed: boolean;
  more: boolean;
}

/** Validate the bounded native replay contract before xterm receives any text. */
export function validateBatch(batch: OutputBatch, cursor: number): string {
  if (!batch || !Array.isArray(batch.frames) || (batch.closed && batch.more)) throw new Error("invalid output");
  if (
    !Number.isSafeInteger(batch.cursor) ||
    batch.cursor < cursor ||
    batch.frames.length > 512 ||
    typeof batch.truncated !== "boolean" ||
    typeof batch.closed !== "boolean" ||
    typeof batch.more !== "boolean"
  )
    throw new Error("invalid output");
  let previous = cursor;
  let bytes = 0;
  for (const frame of batch.frames) {
    if (
      !Number.isSafeInteger(frame.sequence) ||
      frame.sequence <= previous ||
      frame.sequence > batch.cursor ||
      ((previous !== cursor || !batch.truncated) && frame.sequence !== previous + 1) ||
      typeof frame.data !== "string"
    )
      throw new Error("invalid output");
    previous = frame.sequence;
    bytes += new TextEncoder().encode(frame.data).length;
  }
  if (
    bytes > 64 * 1024 ||
    (batch.frames.length > 0 && previous !== batch.cursor) ||
    (batch.frames.length === 0 && batch.cursor !== cursor) ||
    (batch.more && batch.cursor === cursor)
  )
    throw new Error("invalid output");
  return batch.frames.map((frame) => frame.data).join("");
}
