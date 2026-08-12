export type EncodingKind = "utf8" | "utf16Le" | "utf16Be" | "cp949";

export interface Encoding {
  encodingKind: EncodingKind;
  bom: boolean;
}

export type LineEnding = "lf" | "crlf" | "cr";

export interface OpenedFile {
  path: string;
  text: string;
  encoding: Encoding;
  lineEnding: LineEnding;
  readOnly: boolean;
  size: number;
  /** Decimal epoch nanoseconds. Keep this as a string: JS numbers lose i64 precision. */
  mtimeNanos: string;
  /** SHA-256 of the exact bytes read from disk. */
  contentHash: string;
  lossy: boolean;
}

export interface SavedFile {
  path: string;
  /** Decimal epoch nanoseconds. Keep this as a string: JS numbers lose i64 precision. */
  mtimeNanos: string;
  size: number;
  /** SHA-256 of the exact bytes committed; use this for the next save snapshot. */
  contentHash: string;
  /** Present only when the content committed but a durability refresh failed. */
  durabilityWarning: string | null;
}
