import type { RpcClient } from "../../../rpc/client";
import type { LogLine, SearchCursor } from "../../../rpc/gen/rpc";

const MAX_BYTES = 64 * 1024;
const encode = (text: string) => new TextEncoder().encode(text).length;
export function quoteFailureLogs(runId: string, newest: LogLine[]): string {
  const header = `테스트 실행 ${runId}이 실패했습니다. 아래 가려진 실행 로그를 확인해 수정해 주세요.\n`;
  const omitted = "> [전송 크기 제한으로 일부 로그 생략]\n";
  let left = MAX_BYTES - encode(header) - encode(omitted);
  const rows: string[] = [];
  let truncated = newest.length > 200;
  for (const row of newest.slice(0, 200)) {
    const text = `> ${row.text.replaceAll("\0", "�").replaceAll("\n", "\n> ")}\n`;
    if (encode(text) > left) {
      truncated = true;
      if (!rows.length) {
        const tail: string[] = [];
        left -= encode("> ") + 1;
        for (const character of Array.from(row.text.replaceAll("\0", "�").replaceAll("\n", " ")).reverse()) {
          const bytes = encode(character);
          if (bytes > left) break;
          tail.push(character);
          left -= bytes;
        }
        rows.push(`> ${tail.reverse().join("")}\n`);
      }
      break;
    }
    rows.push(text);
    left -= encode(text);
  }
  return header + (truncated ? omitted : "") + rows.reverse().join("");
}

/** Fetch only this run's server-redacted rows, independent of LogView's filters/buffer. */
export async function readFailureLogs(client: RpcClient, runId: string, signal: AbortSignal): Promise<string> {
  const boundedSignal = AbortSignal.any([signal, AbortSignal.timeout(5000)]);
  const rows: LogLine[] = [];
  const seen = new Set<string>();
  let before: SearchCursor | undefined;
  for (let page = 0; page < 256; page++) {
    boundedSignal.throwIfAborted();
    const result = await client.call(
      "logs.search",
      {
        sources: [{ kind: "run", runId }],
        query: "",
        regex: false,
        limit: 200 - rows.length,
        ...(before ? { before } : {}),
      },
      { signal: boundedSignal },
    );
    rows.push(
      ...result.items.filter((row) => row.src.kind === "run" && row.src.runId === runId).slice(0, 200 - rows.length),
    );
    if (rows.length === 200 || !result.nextCursor) return quoteFailureLogs(runId, rows);
    const key = JSON.stringify(result.nextCursor);
    if (seen.has(key)) throw new Error("로그 페이지가 반복되어 전송하지 못했습니다.");
    seen.add(key);
    before = result.nextCursor;
  }
  throw new Error("로그 조회 한도를 초과했습니다. 다시 시도하세요.");
}
