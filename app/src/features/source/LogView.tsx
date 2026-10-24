import { useState } from "react";
import type { SourceTarget } from "../../rpc/gen/rpc";
import { useRpcQuery } from "../../rpc/query";
import { Banner, Button } from "../../ui";
import { gitError } from "./gitState";
export function LogView({ target, path }: { target: SourceTarget; path?: string }) {
  return <TargetLog key={JSON.stringify([target, path])} target={target} path={path} />;
}
function TargetLog({ target, path }: { target: SourceTarget; path?: string }) {
  const [skip, setSkip] = useState(0);
  const query = useRpcQuery("git.log", { target, ...(path ? { path } : {}), limit: 50, skip }, { staleTime: 0 });
  return (
    <section aria-label="커밋 기록">
      <h3>{path ? `${path} · ` : ""}커밋 기록</h3>
      {query.error && <Banner tone="danger">{gitError(query.error)}</Banner>}
      {query.isPending && <p role="status">기록 불러오는 중…</p>}
      <Button onClick={() => void query.refetch()}>기록 새로고침</Button>
      <ol start={skip + 1}>
        {query.data?.map((commit) => (
          <li key={commit.sha}>
            <code>{commit.sha.slice(0, 12)}</code> {commit.subject} · {commit.author} ·{" "}
            {new Date(commit.timeMs).toLocaleString("ko-KR")}
          </li>
        ))}
      </ol>
      <Button disabled={skip === 0 || query.isFetching} onClick={() => setSkip((value) => Math.max(0, value - 50))}>
        최근 기록
      </Button>
      <Button
        disabled={query.data?.length !== 50 || query.isFetching || !!query.error}
        onClick={() => setSkip((value) => value + 50)}
      >
        이전 기록
      </Button>
    </section>
  );
}
