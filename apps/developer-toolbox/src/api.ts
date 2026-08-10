import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";
import type { DiffHunk, RegexMatch } from "./types";

/** 데이터를 해시한다. browser 미리보기에서는 Web Crypto(SHA)만 지원. */
export async function hash(data: string, algorithm: string): Promise<string> {
  if (!isTauri()) return browserHash(data, algorithm);
  return invoke<string>("hash", { data, algorithm });
}

/** UUID v4를 생성한다. */
export async function generateUuid(): Promise<string> {
  if (!isTauri()) return crypto.randomUUID();
  return invoke<string>("generate_uuid");
}

/** 정규식 전체 매치 목록을 반환한다. */
export async function regexTest(pattern: string, text: string): Promise<RegexMatch[]> {
  if (!isTauri()) return browserRegex(pattern, text);
  return invoke<RegexMatch[]>("regex_test", { pattern, text });
}

/** 두 텍스트의 라인 단위 변경 구간을 반환한다. */
export async function diff(a: string, b: string): Promise<DiffHunk[]> {
  if (!isTauri()) return browserDiff(a, b);
  return invoke<DiffHunk[]>("diff", { a, b });
}

async function browserHash(data: string, algorithm: string): Promise<string> {
  if (algorithm.toLowerCase() === "md5") {
    throw new Error("MD5는 브라우저 미리보기에서 지원되지 않습니다. Tauri 앱에서 사용하세요.");
  }
  const buf = await crypto.subtle.digest(
    algorithm.toLowerCase() === "sha512" ? "SHA-512" : "SHA-256",
    new TextEncoder().encode(data),
  );
  return Array.from(new Uint8Array(buf))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

function browserRegex(pattern: string, text: string): RegexMatch[] {
  const re = new RegExp(pattern, "g");
  const out: RegexMatch[] = [];
  for (const m of text.matchAll(re)) {
    out.push({ start: m.index, end: m.index + m[0].length, text: m[0] });
  }
  return out;
}

/** 단순 라인 diff: 공통 접두/접미를 찾아 중간을 replace로 표시. */
function browserDiff(a: string, b: string): DiffHunk[] {
  const al = a.split("\n");
  const bl = b.split("\n");
  let start = 0;
  while (start < al.length && start < bl.length && al[start] === bl[start]) start++;
  let aEnd = al.length;
  let bEnd = bl.length;
  while (aEnd > start && bEnd > start && al[aEnd - 1] === bl[bEnd - 1]) {
    aEnd--;
    bEnd--;
  }
  const hunks: DiffHunk[] = [];
  if (start > 0) hunks.push({ kind: 0, old_start: 0, old_end: start, new_start: 0, new_end: start });
  if (aEnd > start || bEnd > start) {
    hunks.push({ kind: 2, old_start: start, old_end: aEnd, new_start: start, new_end: bEnd });
  }
  if (aEnd < al.length) hunks.push({ kind: 0, old_start: aEnd, old_end: al.length, new_start: bEnd, new_end: bl.length });
  return hunks;
}
