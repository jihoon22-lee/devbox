import type { SourceFile, SourceTarget } from "../../rpc/gen/rpc";
import { useRpcQuery } from "../../rpc/query";
import { Banner, Button } from "../../ui";
import { gitError } from "./gitState";
export type SourceSelection = { path: string; staged: boolean; conflicted: boolean };
export function ChangesList({
  target,
  selected,
  onSelect,
}: {
  target: SourceTarget;
  selected?: SourceSelection;
  onSelect: (selection: SourceSelection) => void;
}) {
  const status = useRpcQuery("git.status", { target }, { staleTime: 0 });
  const files = status.data?.files ?? [];
  const groups: { name: string; staged: boolean; files: SourceFile[] }[] = [
    { name: "충돌", staged: false, files: files.filter((file) => file.conflicted) },
    {
      name: "스테이징됨",
      staged: true,
      files: files.filter(
        (file) => !file.conflicted && file.indexStatus !== "." && file.indexStatus !== " " && file.indexStatus !== "?",
      ),
    },
    {
      name: "스테이징하지 않음",
      staged: false,
      files: files.filter((file) => !file.conflicted && file.worktreeStatus !== "." && file.worktreeStatus !== " "),
    },
  ];
  return (
    <section aria-label="변경 목록">
      <Button onClick={() => void status.refetch()}>변경 목록 새로고침</Button>
      {status.error && <Banner tone="danger">{gitError(status.error)}</Banner>}
      {status.isPending && <p role="status">변경 목록 불러오는 중…</p>}
      {groups.map((group) => (
        <section key={group.name} aria-label={group.name}>
          <h3>
            {group.name} ({group.files.length})
          </h3>
          <ul>
            {group.files.map((file) => (
              <li key={file.path}>
                <Button
                  disabled={!!status.error || status.isFetching}
                  aria-pressed={selected?.path === file.path && selected.staged === group.staged}
                  onClick={() => onSelect({ path: file.path, staged: group.staged, conflicted: file.conflicted })}
                >
                  {file.path}
                  {file.oldPath ? ` ← ${file.oldPath}` : ""}
                </Button>
              </li>
            ))}
          </ul>
        </section>
      ))}
      {status.data?.warnings.map((warning) => (
        <Banner key={warning.key} tone="warn">
          저장소 설정 {warning.key}은 이 작업에서 사용하지 않습니다.
        </Banner>
      ))}
    </section>
  );
}
