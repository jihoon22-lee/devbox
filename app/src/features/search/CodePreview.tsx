import type { SearchHit } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
import { useRpcQuery } from "../../rpc/query";
import { Button } from "../../ui";
import { CodeView } from "../../ui/code/CodeView";
export function CodePreview({ hit, onOpen }: { hit: SearchHit; onOpen?: (hit: SearchHit) => void }) {
  const query = useRpcQuery("documents.read", { key: { root: hit.root, path: hit.path }, policy: { kind: "code" } });
  if (query.error) return <p role="alert">{messageFor(query.error.code, query.error.detail).title}</p>;
  if (!query.data) return <p role="status">파일을 불러오는 중…</p>;
  return (
    <section aria-label="코드 미리보기">
      <h3>
        {hit.path}
        {hit.line ? `:${hit.line}` : ""} · 읽기 전용
      </h3>
      {onOpen && <Button onClick={() => onOpen(hit)}>편집기로 열기</Button>}
      {query.data.text === null ? (
        <p>파일이 없거나, 너무 크거나, 텍스트로 표시할 수 없습니다.</p>
      ) : (
        <CodeView ariaLabel="검색 결과 코드" value={query.data.text} readOnly />
      )}
    </section>
  );
}
