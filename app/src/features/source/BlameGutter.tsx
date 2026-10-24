import type { SourceTarget } from "../../rpc/gen/rpc";
import { useRpcQuery } from "../../rpc/query";
import { Banner, Button } from "../../ui";
import { gitError } from "./gitState";
export function BlameGutter({
  target,
  path,
  onOpen,
}: {
  target: SourceTarget;
  path: string;
  onOpen: (line: number) => void;
}) {
  const query = useRpcQuery("git.blame", { target, path }, { staleTime: 0 });
  return (
    <section aria-label="줄별 변경 기록">
      <h3>{path} · 줄별 변경 기록</h3>
      <Button onClick={() => void query.refetch()}>줄 기록 새로고침</Button>
      {query.error && <Banner tone="danger">{gitError(query.error)}</Banner>}
      {query.isPending && <p role="status">줄 기록 불러오는 중…</p>}
      <ol>
        {query.data?.map((line) => (
          <li key={line.line}>
            <Button onClick={() => onOpen(line.line)}>{line.line}줄</Button> <code>{line.oid.slice(0, 12)}</code>{" "}
            {line.author} · {new Date(line.timeMs).toLocaleDateString("ko-KR")}
            <pre>{line.text}</pre>
          </li>
        ))}
      </ol>
    </section>
  );
}
