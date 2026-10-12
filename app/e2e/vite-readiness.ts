import { stripVTControlCharacters } from "node:util";

export function viteAnnounced(log: string, port: number): boolean {
  // Strip after aggregation: a stream chunk may end halfway through an escape sequence.
  const plain = stripVTControlCharacters(log);
  return new RegExp(
    `^[\\t ]*(?:➜[\\t ]*)?Local:[\\t ]+http://(?:localhost|127\\.0\\.0\\.1):${port}/[\\t ]*\\r?$`,
    "m",
  ).test(plain);
}
