import { useRpcQuery } from "../../../rpc/query";
import { EmptyState } from "../../../ui";
import s from "./Detail.module.css";
import { QueryError } from "./QueryError";
export function CommitsTab({ agentId }: { agentId: string }) {
  const commits = useRpcQuery("agents.commits", { id: agentId });
  return (
    <>
      <QueryError query={commits} />
      {!commits.data && !commits.error && <p role="status">커밋을 불러오는 중…</p>}
      {commits.data?.items.length === 0 && <EmptyState title="아직 커밋이 없습니다" />}
      <ol className={s.commits}>
        {commits.data?.items.map((commit) => (
          <li key={commit.sha}>
            <code>{commit.sha.slice(0, 8)}</code> · {commit.subject}
            <br />
            <span>
              {commit.author} · {new Date(commit.timeMs).toLocaleString("ko")}
            </span>
          </li>
        ))}
      </ol>
    </>
  );
}
