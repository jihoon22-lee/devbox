import { useState } from "react";
import { WorkspaceOperationError } from "../../transport";
import { repoFileHunks, repoHunksApply, type RepoEntry, type FileHunks, type HunkAction } from "../api";
import { useSourceAction } from "./useSourceAction";
interface Props {
  repo: RepoEntry;
  file: string;
  staged: boolean;
  onChanged?: () => void;
  onBusyChange?: (busy: boolean) => void;
  disabled?: boolean;
}
export default function HunkList({ repo, file, staged, onChanged, onBusyChange, disabled = false }: Props) {
  const identity = JSON.stringify([repo.canonicalKey, repo.path, file, staged]);
  const [opened, setOpened] = useState<string | null>(null);
  const [snapshot, setSnapshot] = useState<{ identity: string; value: FileHunks } | null>(null);
  const [pending, setPending] = useState<{ identity: string; id: string; revision: string } | null>(null);
  const { busy, issue, execute } = useSourceAction(repo, onBusyChange);
  const view = snapshot?.identity === identity ? snapshot.value : null;
  const confirmation = pending?.identity === identity ? pending : null;
  const load = async (signal: AbortSignal) => {
    const value = await repoFileHunks(repo.path, file, staged);
    if (!signal.aborted) {
      setSnapshot({ identity, value });
      setPending(null);
    }
  };
  const apply = async (action: HunkAction, id: string, revision: string) => {
    const done = await execute(async (operationId, signal) => {
      try {
        await repoHunksApply(repo.path, file, staged, action, [id], revision, operationId);
      } catch (cause) {
        if (!signal.aborted && cause instanceof WorkspaceOperationError && cause.code === "hunk_stale")
          await load(signal);
        throw cause;
      }
      await load(signal);
      return true;
    });
    if (done) onChanged?.();
  };
  const toggle = () => {
    if (opened === identity) {
      setOpened(null);
      setPending(null);
      return;
    }
    setOpened(identity);
    void execute((_id, signal) => load(signal));
  };
  return (
    <div className="source-hunks">
      <button type="button" disabled={disabled || busy} onClick={toggle}>
        {opened === identity ? "변경 덩어리 닫기" : "변경 덩어리 보기"}
      </button>
      {opened === identity && (
        <>
          {issue && <p role="alert">{issue}</p>}
          {view && !view.supported && (
            <p>
              <span>이 파일은 파일 단위로만 처리할 수 있습니다.</span>
              {view.reason === "binary" && <span>binary 파일</span>}
            </p>
          )}
          {view?.supported && view.hunks.length === 0 && <p>표시할 변경 덩어리가 없습니다.</p>}
          {view?.supported &&
            view.hunks.map((hunk) => (
              <div key={hunk.id} className="source-hunk">
                <code>{hunk.header}</code>
                <pre>
                  {hunk.lines.map((line, index) => (
                    <span key={`${hunk.id}:${index}`} className={`source-hunk-${line.kind}`}>
                      {({ context: " ", add: "+", remove: "-", noNewline: "\\" } as const)[line.kind]}
                      {line.text}
                      {"\n"}
                    </span>
                  ))}
                </pre>
                <button
                  type="button"
                  disabled={disabled || busy}
                  onClick={() => void apply(staged ? "unstage" : "stage", hunk.id, view.revision)}
                >
                  {staged ? "이 덩어리 unstage" : "이 덩어리 stage"}
                </button>
                {!staged && (
                  <button
                    type="button"
                    disabled={disabled || busy}
                    onClick={() => setPending({ identity, id: hunk.id, revision: view.revision })}
                  >
                    이 덩어리 버리기
                  </button>
                )}
              </div>
            ))}
          {confirmation && (
            <div role="group" aria-label="변경 버리기 확인">
              <p>이 변경을 버립니다. 되돌릴 수 없습니다.</p>
              <button
                type="button"
                disabled={disabled || busy}
                onClick={() => void apply("discard", confirmation.id, confirmation.revision)}
              >
                변경 버리기
              </button>
              <button type="button" disabled={disabled || busy} onClick={() => setPending(null)}>
                버리기 취소
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}
