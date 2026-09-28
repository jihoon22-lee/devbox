import { useEffect, useState } from "react";
import {
  repoConflictVersions,
  repoConflictResolve,
  repoOperationContinue,
  repoOperationAbort,
  type RepoEntry,
  type ConflictState,
  type ConflictVersions,
  type Resolution,
} from "../api";
import { useSourceAction } from "./useSourceAction";
import type { ConflictKind } from "../../generated/ConflictKind";
const choices: Record<ConflictKind, Resolution["kind"][]> = {
  bothModified: ["ours", "theirs", "content"],
  bothAdded: ["ours", "theirs", "content"],
  bothDeleted: ["delete"],
  addedByUs: ["ours", "delete"],
  addedByThem: ["theirs", "delete"],
  deletedByUs: ["theirs", "delete"],
  deletedByThem: ["ours", "delete"],
};
const labels = {
  ours: "우리 쪽 사용",
  theirs: "상대 쪽 사용",
  content: "편집한 결과로 해결",
  delete: "파일 삭제로 해결",
};
const operations = { merge: "병합", rebase: "rebase", cherryPick: "cherry-pick", revert: "revert" };
interface Props {
  repo: RepoEntry;
  state: ConflictState;
  onChanged?: () => void;
  onBusyChange?: (busy: boolean) => void;
  onDirtyChange?: (dirty: boolean) => void;
}
export default function ConflictPanel({ repo, state, onChanged, onBusyChange, onDirtyChange }: Props) {
  const { busy, issue, execute } = useSourceAction(repo, onBusyChange);
  const [selected, setSelected] = useState<{ file: string; versions: ConflictVersions } | null>(null);
  const [text, setText] = useState("");
  const [confirmAbort, setConfirmAbort] = useState(false);
  const file = state.files.find((file) => file.path === selected?.file);
  const dirty = !!file && selected !== null && text !== (selected.versions.current ?? "");
  useEffect(() => {
    onDirtyChange?.(dirty);
  }, [dirty, onDirtyChange]);
  useEffect(() => () => onDirtyChange?.(false), [onDirtyChange]);
  const markers = text
    .split(/\r?\n/)
    .some((line) => line.startsWith("<<<<<<< ") || line === "=======" || line.startsWith(">>>>>>> "));
  const load = (path: string) =>
    void execute(async (_id, signal) => {
      const versions = await repoConflictVersions(repo.path, path);
      if (!signal.aborted) {
        setSelected({ file: path, versions });
        setText(versions.current ?? "");
      }
    });
  const resolve = (kind: Resolution["kind"]) =>
    void execute(async (id, signal) => {
      if (!file) return;
      await repoConflictResolve(repo.path, file.path, kind === "content" ? { kind, text } : { kind }, id);
      if (!signal.aborted) {
        setSelected(null);
        setText("");
        onChanged?.();
      }
    });
  const finish = (abort: boolean) =>
    void execute(async (id, signal) => {
      await (abort ? repoOperationAbort : repoOperationContinue)(repo.path, id);
      if (!signal.aborted) {
        setSelected(null);
        setText("");
        setConfirmAbort(false);
        onChanged?.();
      }
    });
  return (
    <section aria-label="충돌 해결" className="source-conflict-panel">
      <h2>
        {state.operation ? operations[state.operation] : "stash 적용"} 중 충돌 {state.files.length}개
      </h2>
      {issue && <p role="alert">{issue}</p>}
      <ul>
        {state.files.map((file) => (
          <li key={file.path}>
            <button type="button" disabled={busy || dirty} onClick={() => load(file.path)}>
              {file.path}
            </button>
          </li>
        ))}
      </ul>
      {file && selected && (
        <div>
          <h3>{file.path}</h3>
          {selected.versions.binary ? (
            <p>바이너리 또는 큰 파일입니다. 사용할 쪽을 선택해 주세요.</p>
          ) : (
            <>
              <div className="source-conflict-versions">
                {[
                  ["공통 조상", selected.versions.base],
                  ["우리 쪽", selected.versions.ours],
                  ["상대 쪽", selected.versions.theirs],
                ].map(([label, value]) => (
                  <label key={label}>
                    {label}
                    <textarea readOnly value={value ?? "(파일 없음)"} />
                  </label>
                ))}
              </div>
              {choices[file.kind].includes("content") && (
                <label>
                  결과
                  <textarea disabled={busy} value={text} onChange={(event) => setText(event.target.value)} />
                </label>
              )}
              {markers && (
                <p>{"충돌 표시(<<<<<<<, =======, >>>>>>>)가 남아 있습니다. 모두 정리한 뒤 해결해 주세요."}</p>
              )}
            </>
          )}
          {choices[file.kind]
            .filter((kind) => !(kind === "content" && selected.versions.binary))
            .map((kind) => (
              <button
                key={kind}
                type="button"
                disabled={busy || (kind === "content" && markers)}
                onClick={() => resolve(kind)}
              >
                {labels[kind]}
              </button>
            ))}
        </div>
      )}
      <button type="button" disabled={busy || !state.operation || state.files.length > 0} onClick={() => finish(false)}>
        계속
      </button>
      <button
        type="button"
        disabled={busy || (!state.operation && state.files.length === 0)}
        onClick={() => setConfirmAbort(true)}
      >
        중단
      </button>
      {confirmAbort && (
        <div role="group" aria-label="Git 작업 중단 확인">
          <p>진행 중인 해결을 모두 버리고 작업 전으로 돌아갑니다.</p>
          <button type="button" disabled={busy} onClick={() => finish(true)}>
            중단 확인
          </button>
          <button type="button" disabled={busy} onClick={() => setConfirmAbort(false)}>
            중단 취소
          </button>
        </div>
      )}
    </section>
  );
}
