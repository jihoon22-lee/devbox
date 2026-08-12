import { invoke } from "@tauri-apps/api/core";
import type { Encoding, LineEnding, OpenedFile, SavedFile } from "./types";

export function openFile(path: string, encoding: Encoding | null = null): Promise<OpenedFile> {
  return invoke<OpenedFile>("open_file", { request: { path, encoding } });
}

export function saveFile(
  path: string,
  text: string,
  encoding: Encoding,
  lineEnding: LineEnding,
  expectedMtimeNanos: string,
  expectedSize: number,
  expectedContentHash: string,
  sourceLossy: boolean,
): Promise<SavedFile> {
  return invoke<SavedFile>("save_file", {
    request: {
      path,
      text,
      encoding,
      lineEnding,
      expectedMtimeNanos,
      expectedSize,
      expectedContentHash,
      sourceLossy,
    },
  });
}
