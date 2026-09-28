import { useCallback, useEffect, useState } from "react";
import { useUndo } from "@devbox/product-shell/undo";
import { WorkspaceOperationError } from "../../transport";
import {
  repoBranches,
  repoBranchCreate,
  repoSwitch,
  repoBranchRename,
  repoBranchDelete,
  repoStashPush,
  type RepoEntry,
  type BranchList,
  type Branch,
} from "../api";
import { useSourceAction } from "./useSourceAction";
interface Props {
  repo: RepoEntry;
  onBusyChange?: (busy: boolean) => void;
  onChanged?: () => void;
}
export default function BranchPanel({ repo, onBusyChange, onChanged }: Props) {
  const [list, setList] = useState<BranchList | null>(null);
  const [name, setName] = useState("");
  const [checkout, setCheckout] = useState(true);
  const [rename, setRename] = useState<{ from: string; to: string } | null>(null);
  const [blocked, setBlocked] = useState(false);
  const [refreshFailed, setRefreshFailed] = useState(false);
  const { busy, issue, execute, scopedUndo } = useSourceAction(repo, onBusyChange);
  const { offer, toast } = useUndo();
  const load = useCallback(
    async (signal: AbortSignal) => {
      const next = await repoBranches(repo.path);
      if (!signal.aborted) {
        setList(next);
        setRefreshFailed(false);
      }
    },
    [repo.path],
  );
  useEffect(() => {
    setList(null);
    setBlocked(false);
    setName("");
    setRename(null);
    void execute((_id, signal) => load(signal));
  }, [execute, load]);
  const updated = async (signal: AbortSignal) => {
    await load(signal);
    if (!signal.aborted) onChanged?.();
  };
  const switchBranch = (branch: string) =>
    void execute(async (id, signal) => {
      setBlocked(false);
      try {
        await repoSwitch(repo.path, branch, id);
        await updated(signal);
      } catch (cause) {
        if (!signal.aborted && cause instanceof WorkspaceOperationError && cause.code === "switch_blocked_by_changes")
          setBlocked(true);
        throw cause;
      }
    });
  const remove = async (branch: string) => {
    const deleted = await execute(async (id, signal) => {
      const receipt = await repoBranchDelete(repo.path, branch, id);
      await updated(signal).catch(() => {
        if (!signal.aborted) {
          setList(null);
          setRefreshFailed(true);
        }
      });
      return receipt;
    });
    if (!deleted) return;
    offer(
      `branch '${deleted.name}'를 삭제했습니다.`,
      scopedUndo(async (undoId, signal) => {
        await repoBranchCreate(repo.path, { name: deleted.name, startPoint: deleted.commit, checkout: false }, undoId);
        await updated(signal);
      }),
    );
  };
  const row = (branch: Branch) => {
    const current = branch.name === list?.current && !branch.remote;
    const elsewhere = !!branch.worktree && !current;
    return (
      <li key={branch.name}>
        <span>{branch.name}</span>
        {current && <span>현재</span>}
        {branch.upstream && <span>{`앞섬 ${branch.ahead} · 뒤처짐 ${branch.behind}`}</span>}
        {elsewhere && <span>다른 작업 폴더에서 사용 중</span>}
        {!current && !elsewhere && (
          <button
            type="button"
            disabled={busy}
            aria-label={`${branch.name} ${branch.remote ? "추적 branch로 전환" : "전환"}`}
            onClick={() => switchBranch(branch.name)}
          >
            {branch.remote ? "추적 branch로 전환" : "전환"}
          </button>
        )}
        {!branch.remote && !elsewhere && (
          <button
            type="button"
            disabled={busy}
            aria-label={`${branch.name} 이름 바꾸기`}
            onClick={() => setRename({ from: branch.name, to: branch.name })}
          >
            이름 바꾸기
          </button>
        )}
        {!branch.remote && !current && !elsewhere && (
          <button type="button" disabled={busy} aria-label={`${branch.name} 삭제`} onClick={() => remove(branch.name)}>
            삭제
          </button>
        )}
      </li>
    );
  };
  return (
    <section aria-label="Branch" className="source-branch-panel">
      <h2>Branch</h2>
      <button type="button" disabled={busy} onClick={() => void execute((_id, signal) => load(signal))}>
        branch 새로 고침
      </button>
      {issue && <p role="alert">{issue}</p>}
      {refreshFailed && <p role="alert">삭제는 완료했지만 목록을 읽지 못했습니다. 새로 고쳐 주세요.</p>}
      {blocked && (
        <button
          type="button"
          disabled={busy}
          onClick={() =>
            void execute(async (id, signal) => {
              await repoStashPush(repo.path, { message: null, includeUntracked: false }, id);
              if (!signal.aborted) setBlocked(false);
              await updated(signal);
            })
          }
        >
          변경을 stash에 저장
        </button>
      )}
      {list?.detached && <p>현재 branch 없음(detached)</p>}
      {list?.truncated && <p>branch가 많아 일부만 표시합니다.</p>}
      <ul aria-label="로컬 branch">{list?.branches.filter((branch) => !branch.remote).map(row)}</ul>
      <details>
        <summary>원격 branch</summary>
        <ul>{list?.branches.filter((branch) => branch.remote).map(row)}</ul>
      </details>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void execute(async (id, signal) => {
            await repoBranchCreate(repo.path, { name: name.trim(), startPoint: null, checkout }, id);
            if (!signal.aborted) setName("");
            await updated(signal);
          });
        }}
      >
        <label>
          branch 이름
          <input value={name} disabled={busy} onChange={(event) => setName(event.target.value)} required />
        </label>
        <label>
          <input
            type="checkbox"
            checked={checkout}
            disabled={busy}
            onChange={(event) => setCheckout(event.target.checked)}
          />
          만든 뒤 전환
        </label>
        <button disabled={busy || !name.trim()}>새 branch 만들기</button>
      </form>
      {rename && (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void execute(async (id, signal) => {
              await repoBranchRename(repo.path, rename.from, rename.to.trim(), id);
              if (!signal.aborted) setRename(null);
              await updated(signal);
            });
          }}
        >
          <label>
            새 branch 이름
            <input
              value={rename.to}
              disabled={busy}
              onChange={(event) => setRename({ ...rename, to: event.target.value })}
              required
            />
          </label>
          <button disabled={busy || !rename.to.trim()}>이름 변경 적용</button>
          <button type="button" disabled={busy} onClick={() => setRename(null)}>
            이름 변경 취소
          </button>
        </form>
      )}
      {toast}
    </section>
  );
}
