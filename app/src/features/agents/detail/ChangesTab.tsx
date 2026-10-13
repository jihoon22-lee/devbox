import { useMemo, useState } from "react";
import { useRpcQuery } from "../../../rpc/query";
import { Button, EmptyState } from "../../../ui";
import { DiffView } from "../../../ui/DiffView";
import { type DiffRange, parseUnifiedDiff } from "../../../ui/diff";
import s from "./Detail.module.css";
import { useDrafts } from "./drafts";
import { QueryError } from "./QueryError";
import { quoteForPrompt } from "./quote";
import { useSeen } from "./seen";

const labels = { added: "A", modified: "M", deleted: "D", renamed: "R", unmerged: "U" };
export function ChangesTab({ agentId, closed, onQuote }: { agentId: string; closed?: boolean; onQuote?: () => void }) {
  const changes = useRpcQuery("agents.changes", { id: agentId });
  const [selected, setSelected] = useState<string>();
  const entries = useSeen((state) => state.entries);
  const [selection, setSelection] = useState<{ path: string; patch: string; range: DiffRange }>();
  const files = changes.data?.files ?? [];
  const current = files.find((file) => file.path === selected) ?? files[0];
  const path = current?.path ?? "";
  const patch = useRpcQuery("agents.patch", { id: agentId, path }, { enabled: !!current });
  const reviewing = changes.isFetching || patch.isFetching;
  const stale = reviewing || !!changes.error || !!patch.error;
  const parsed = useMemo(() => parseUnifiedDiff(patch.data?.patch ?? ""), [patch.data?.patch]);
  const seen = (file: (typeof files)[number]) =>
    entries[JSON.stringify([agentId, file.path])] === `${file.status}:${file.added}:${file.removed}`;
  const range = selection?.path === path && selection.patch === patch.data?.patch ? selection.range : undefined;
  return (
    <>
      <QueryError query={changes} />
      {changes.data && (
        <p>
          기준 {changes.data.base} 대비 · 앞섬 {changes.data.ahead} · 뒤처짐 {changes.data.behind}
        </p>
      )}
      {!changes.data && !changes.error && <p role="status">변경 내용을 불러오는 중…</p>}
      {changes.data && !files.length && <EmptyState title="변경한 파일이 없습니다" />}
      {!!files.length && (
        <div className={s.changes}>
          <nav className={s.files} aria-label="변경한 파일">
            {files.map((file) => (
              <button
                key={file.path}
                type="button"
                className={s.file}
                aria-current={file.path === path ? "true" : undefined}
                onClick={() => setSelected(file.path)}
              >
                {labels[file.status]} {file.path} · +{file.added} −{file.removed} {seen(file) && "✓ 봤음"}
              </button>
            ))}
          </nav>
          <div className={s.patch}>
            {current && (
              <Button
                size="sm"
                disabled={stale}
                onClick={() => {
                  useSeen.getState().mark(agentId, path, `${current.status}:${current.added}:${current.removed}`);
                  const next = files.find((file) => file.path !== path && !seen(file));
                  if (next) setSelected(next.path);
                }}
              >
                봤음
              </Button>
            )}
            {range && !closed && (
              <Button
                size="sm"
                disabled={stale}
                onClick={() => {
                  const quotePath = range.side === "old" ? (current?.oldPath ?? path) : path;
                  useDrafts.getState().append(agentId, quoteForPrompt(quotePath, range));
                  onQuote?.();
                }}
              >
                지시에 인용
              </Button>
            )}
            <QueryError query={patch} />
            {stale && patch.data && (
              <p role="status">
                {reviewing
                  ? "최신 변경 내용을 확인하는 중입니다. 표시된 내용은 이전 조회 결과입니다."
                  : "최신 변경 내용을 확인하지 못했습니다. 표시된 내용은 이전 조회 결과입니다."}
              </p>
            )}
            {!patch.data && !patch.error && <p role="status">파일 변경 내용을 불러오는 중…</p>}
            {patch.data?.truncated && <p role="status">파일 변경 내용이 너무 길어 일부만 가져왔습니다.</p>}
            {parsed.map((file, index) => (
              <DiffView
                key={`${path}:${index}`}
                file={file}
                onSelectLines={(range) => setSelection({ path, patch: patch.data!.patch, range })}
              />
            ))}
            {patch.data && !patch.data.patch && <p>표시할 텍스트 변경이 없습니다.</p>}
          </div>
        </div>
      )}
    </>
  );
}
