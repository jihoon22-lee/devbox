import { useRpcQuery, useTopicInvalidation } from "../../rpc/query";
import { Button, ErrorState } from "../../ui";
import s from "./notes.module.css";

/** Notes that link here (`notes.backlinks`); choosing one opens it. */
export function Backlinks({ path, onOpen }: { path: string; onOpen(path: string): void }) {
  useTopicInvalidation("notes.changed", [["notes.backlinks"]]);
  const links = useRpcQuery("notes.backlinks", { path });
  return (
    <aside className={s.backlinks} aria-label="백링크">
      <h3 className={s.panelTitle}>백링크 {links.data ? links.data.items.length : ""}</h3>
      {links.isError && (
        <ErrorState
          title="백링크를 읽지 못했습니다"
          action={<Button onClick={() => void links.refetch()}>다시 시도</Button>}
        />
      )}
      {links.data && !links.data.items.length && <p className={s.notice}>이 노트를 가리키는 노트가 없습니다.</p>}
      <ul>
        {links.data?.items.map((item) => (
          <li key={`${item.src}:${item.line}:${item.fromUtf16}`}>
            <button type="button" className={s.linkButton} onClick={() => onOpen(item.src)}>
              {item.title || item.src}
              <span className={s.count}>{item.line}줄</span>
            </button>
          </li>
        ))}
      </ul>
      {links.data?.truncated && <p className={s.notice}>앞의 256개만 보입니다.</p>}
    </aside>
  );
}
