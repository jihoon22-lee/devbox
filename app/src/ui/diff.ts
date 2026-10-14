export interface DiffLine {
  kind: "ctx" | "add" | "del" | "meta";
  old?: number;
  new?: number;
  text: string;
}
export interface DiffFile {
  oldPath: string | null;
  newPath: string | null;
  binary: boolean;
  hunks: { header: string; lines: DiffLine[] }[];
}
export interface DiffRange {
  side: "old" | "new";
  from: number;
  to: number;
  text: string;
}
/** Git quotes paths as C strings, including octal-escaped UTF-8 bytes. */
function decodePath(path: string) {
  if (!path.startsWith('"')) return path.split("\t")[0] ?? "";
  const bytes: number[] = [];
  const escapes: Record<string, number> = { a: 7, b: 8, t: 9, n: 10, v: 11, f: 12, r: 13 };
  for (let i = 1; i < path.length; i++) {
    const char = path[i];
    if (char === '"') break;
    if (char !== "\\") {
      const code = path.codePointAt(i)!;
      bytes.push(...new TextEncoder().encode(String.fromCodePoint(code)));
      if (code > 0xffff) i++;
    } else {
      const octal = /^[0-7]{1,3}/.exec(path.slice(i + 1));
      if (octal) {
        bytes.push(Number.parseInt(octal[0], 8));
        i += octal[0].length;
      } else {
        i++;
        bytes.push(escapes[path[i] ?? ""] ?? (path[i] ? path.charCodeAt(i) : 92));
      }
    }
  }
  return new TextDecoder().decode(new Uint8Array(bytes));
}
const strip = (path: string) => {
  const decoded = decodePath(path);
  return decoded === "/dev/null" ? null : decoded.replace(/^[ab]\//, "");
};
function headerPaths(raw: string): [string | null, string | null] {
  const paths = raw.slice(11);
  const quoted = /^("(?:[^"\\]|\\.)*"|a\/.*?) ("(?:[^"\\]|\\.)*"|b\/.*)$/.exec(paths);
  return quoted ? [strip(quoted[1]!), strip(quoted[2]!)] : [null, null];
}
export function parseUnifiedDiff(text: string): DiffFile[] {
  const files: DiffFile[] = [];
  let file: DiffFile | undefined;
  let hunk: DiffFile["hunks"][number] | undefined;
  let old = 0,
    next = 0,
    oldLeft = 0,
    newLeft = 0;
  for (const raw of text.split("\n")) {
    if (hunk && raw.startsWith("\\")) {
      hunk.lines.push({ kind: "meta", text: raw });
      continue;
    }
    if (hunk && (oldLeft > 0 || newLeft > 0)) {
      if (raw.startsWith(" ") && oldLeft > 0 && newLeft > 0) {
        hunk.lines.push({ kind: "ctx", old: old++, new: next++, text: raw.slice(1) });
        oldLeft--;
        newLeft--;
        continue;
      }
      if (raw.startsWith("-") && oldLeft > 0) {
        hunk.lines.push({ kind: "del", old: old++, text: raw.slice(1) });
        oldLeft--;
        continue;
      }
      if (raw.startsWith("+") && newLeft > 0) {
        hunk.lines.push({ kind: "add", new: next++, text: raw.slice(1) });
        newLeft--;
        continue;
      }
    }
    if (raw.startsWith("diff --git ")) {
      const [oldPath, newPath] = headerPaths(raw);
      file = { oldPath, newPath, binary: false, hunks: [] };
      files.push(file);
      hunk = undefined;
      oldLeft = 0;
      newLeft = 0;
      continue;
    }
    if (!file) continue;
    const match = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/.exec(raw);
    if (match) {
      old = Number(match[1]);
      next = Number(match[3]);
      oldLeft = Number(match[2] ?? 1);
      newLeft = Number(match[4] ?? 1);
      hunk = { header: raw, lines: [] };
      file.hunks.push(hunk);
    } else if (raw.startsWith("--- ")) file.oldPath = strip(raw.slice(4));
    else if (raw.startsWith("+++ ")) file.newPath = strip(raw.slice(4));
    else if (raw.startsWith("rename from ")) file.oldPath = decodePath(raw.slice(12));
    else if (raw.startsWith("rename to ")) file.newPath = decodePath(raw.slice(10));
    else if (raw.startsWith("Binary files ") || raw === "GIT binary patch") file.binary = true;
  }
  return files;
}
/** Never claim that omitted context or the opposite side belongs to a range. */
export function selectDiffRange(
  lines: DiffLine[],
  side: DiffRange["side"],
  from: number,
  to: number,
): DiffRange | null {
  const start = Math.min(from, to),
    end = Math.max(from, to);
  const selected = lines.filter((line) => line[side] !== undefined && line[side]! >= start && line[side]! <= end);
  if (start < 1 || selected.length !== end - start + 1) return null;
  return { side, from: start, to: end, text: selected.map((line) => line.text).join("\n") };
}
