import type { SourceTarget } from "../../rpc/gen/rpc";
import { useRpcQuery } from "../../rpc/query";
import { useRpc } from "../../rpc/RpcProvider";
import { Banner, Button } from "../../ui";
import { DiffView } from "../../ui/DiffView";
import { type DiffRange, parseUnifiedDiff } from "../../ui/diff";
import { gitError, requestGit, runGit, sourceTargetKey, useSourceGit } from "./gitState";
import s from "./source.module.css";

type QueriedDiffProps = {
  target: SourceTarget;
  path: string;
  staged: boolean;
  mode?: "source" | "review";
  onSelectLines?: (range: DiffRange) => void;
};
type ReviewDiffProps = {
  mode: "review";
  path: string;
  snapshot: { patch: string };
  onSelectLines?: (range: DiffRange) => void;
};
export type SourceDiffProps = QueriedDiffProps | ReviewDiffProps;
export function SourceDiff(props: SourceDiffProps) {
  if ("snapshot" in props)
    return (
      <section aria-label="변경 검토">
        <DiffContents path={props.path} patch={props.snapshot.patch} onSelectLines={props.onSelectLines} />
      </section>
    );
  return <SelectedDiff key={JSON.stringify([props.target, props.path, props.staged])} {...props} />;
}
function SelectedDiff({ target, path, staged, mode = "source", onSelectLines }: QueriedDiffProps) {
  const client = useRpc();
  const diff = useRpcQuery("git.diff", { target, path, staged }, { staleTime: 0 });
  const busy = useSourceGit(
    (state) => state.entries[sourceTargetKey(target)]?.busy || !!state.entries[sourceTargetKey(target)]?.uncertain,
  );
  const disabled = !diff.data || diff.isFetching || !!diff.error || busy;
  const change = (hunk?: string) => {
    if (disabled || !diff.data) return;
    void runGit(client, {
      method: staged ? "git.unstage" : "git.stage",
      params: { target, path, ...(hunk ? { hunk } : {}), expect: diff.data.expect },
    });
  };
  return (
    <section className={s.gitPanel} aria-label="소스 변경 내용">
      <h3>
        {path} · {staged ? "스테이징됨" : "작업 파일"}
      </h3>
      <Button onClick={() => void diff.refetch()}>변경 다시 읽기</Button>
      {diff.error && <Banner tone="danger">{gitError(diff.error)}</Banner>}
      {diff.isPending && <p role="status">변경 불러오는 중…</p>}
      {mode === "source" && (
        <div className={s.toolbar}>
          <Button disabled={disabled} onClick={() => change()}>
            {staged ? "파일 스테이징 해제" : "파일 스테이징"}
          </Button>
          {!staged && (
            <Button
              disabled={disabled}
              onClick={() => {
                if (diff.data)
                  requestGit({ method: "git.discard", params: { target, path, expect: diff.data.expect } });
              }}
            >
              변경 버리기
            </Button>
          )}
          {!diff.data?.fileOnly &&
            diff.data?.hunks.map((hunk, index) => (
              <Button key={hunk.id} disabled={disabled} onClick={() => change(hunk.id)}>
                변경 묶음 {index + 1} {staged ? "스테이징 해제" : "스테이징"}
              </Button>
            ))}
          {diff.data?.fileOnly && <p>이 변경은 파일 단위로 처리합니다.</p>}
        </div>
      )}
      {diff.data && <DiffContents path={path} patch={diff.data.patch} onSelectLines={onSelectLines} />}
    </section>
  );
}

function DiffContents({
  path,
  patch,
  onSelectLines,
}: {
  path: string;
  patch: string;
  onSelectLines?: (range: DiffRange) => void;
}) {
  return (
    <>
      {parseUnifiedDiff(patch).map((file, index) => (
        <DiffView key={`${path}:${index}`} file={file} onSelectLines={onSelectLines} />
      ))}
      {!patch && <p>표시할 텍스트 변경이 없습니다.</p>}
    </>
  );
}
