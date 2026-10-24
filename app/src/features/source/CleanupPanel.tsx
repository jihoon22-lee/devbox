import { useState } from "react";
import type { SourceTarget } from "../../rpc/gen/rpc";
import { useRpcQuery } from "../../rpc/query";
import { Banner, Button } from "../../ui";
import { gitError, requestGit, sourceTargetKey, useSourceGit } from "./gitState";
export function CleanupPanel({ target }: { target: SourceTarget }) {
  const candidates = useRpcQuery("git.cleanup_candidates", { target }, { staleTime: 0 });
  const trees = useRpcQuery("git.worktrees", { target }, { staleTime: 0 });
  const [selected, setSelected] = useState<string[]>([]);
  const entry = useSourceGit((state) => state.entries[sourceTargetKey(target)]);
  const items = candidates.data ?? [];
  const chosen = items.filter((item) => selected.includes(JSON.stringify([item.branch, item.path])));
  const disabled = entry?.busy || !!entry?.uncertain || candidates.isFetching || !!candidates.error || !chosen.length;
  return (
    <section aria-label="작업 폴더 정리">
      <h3>작업 폴더 정리</h3>
      <p>Devbox가 소유하고, 변경 사항이 없으며 현재 브랜치에 병합된 대상만 선택할 수 있습니다.</p>
      <Button
        onClick={() => {
          void candidates.refetch();
          void trees.refetch();
        }}
      >
        정리 목록 새로고침
      </Button>
      {(candidates.error || trees.error) && <Banner tone="danger">{gitError(candidates.error ?? trees.error)}</Banner>}
      {entry?.error && <Banner tone="danger">정리 실패 목록을 유지합니다. {entry.error}</Banner>}
      {entry?.cleanupFailure && (
        <details open>
          <summary>마치지 못한 정리 요청 · 선택 목록 보존</summary>
          <p>일부 대상은 이미 처리되었을 수 있습니다. 새 목록과 비교하세요.</p>
          <ul>
            {entry.cleanupFailure.map((item) => (
              <li key={JSON.stringify([item.branch, item.path])}>
                {item.branch} · {item.path ?? "브랜치만"} · {item.oid}
              </li>
            ))}
          </ul>
        </details>
      )}
      {items.map((item) => {
        const key = JSON.stringify([item.branch, item.path]);
        return (
          <label key={key}>
            <input
              type="checkbox"
              checked={selected.includes(key)}
              onChange={(event) =>
                setSelected((old) => (event.target.checked ? [...old, key] : old.filter((value) => value !== key)))
              }
            />
            {item.branch} · {item.path ?? "브랜치만"} · {item.oid}
          </label>
        );
      })}
      {trees.data
        ?.filter((tree) => !items.some((item) => item.path === tree.path))
        .map((tree) => (
          <label key={tree.path}>
            <input type="checkbox" disabled />
            {tree.branch ?? "분리된 HEAD"} · {tree.path} · 보존
          </label>
        ))}
      {candidates.data?.length === 0 && <p>정리할 수 있는 대상이 없습니다.</p>}
      <Button
        disabled={disabled}
        onClick={() => requestGit({ method: "git.cleanup", params: { target, candidates: chosen } })}
      >
        선택한 대상 정리
      </Button>
    </section>
  );
}
