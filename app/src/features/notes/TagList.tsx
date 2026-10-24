import { useRpcQuery, useTopicInvalidation } from "../../rpc/query";
import { Button, EmptyState, ErrorState } from "../../ui";
import s from "./notes.module.css";

/** Tags in the vault with how many notes carry each (`notes.tags`). */
export function TagList() {
  useTopicInvalidation("notes.changed", [["notes.tags"]]);
  const tags = useRpcQuery("notes.tags", {});
  if (tags.isError)
    return (
      <ErrorState
        title="태그를 읽지 못했습니다"
        action={<Button onClick={() => void tags.refetch()}>다시 시도</Button>}
      />
    );
  if (tags.data && !tags.data.items.length)
    return <EmptyState title="태그가 없습니다" body="노트 frontmatter의 tags에 적은 태그가 여기에 보입니다." />;
  return (
    <ul className={s.tags} aria-label="태그">
      {tags.data?.items.map((item) => (
        <li key={item.tag}>
          <span>#{item.tag}</span>
          <span className={s.count}>{item.count}</span>
        </li>
      ))}
      {tags.data?.truncated && <li className={s.notice}>태그가 많아 앞의 256개만 보입니다.</li>}
    </ul>
  );
}
