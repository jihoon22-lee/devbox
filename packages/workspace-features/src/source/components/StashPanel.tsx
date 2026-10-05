import { useCallback, useEffect, useState } from "react";
import { useUndo } from "@devbox/product-shell/undo";
import {
  repoStashList,
  repoStashPush,
  repoStashApply,
  repoStashDrop,
  repoStashStore,
  type RepoEntry,
  type StashEntry,
} from "../api";
import { useSourceAction } from "./useSourceAction";
interface Props {
  repo: RepoEntry;
  onBusyChange?: (busy: boolean) => void;
  onChanged?: () => void;
}
export default function StashPanel({ repo, onBusyChange, onChanged }: Props) {
  const [entries, setEntries] = useState<StashEntry[]>([]);
  const [message, setMessage] = useState("");
  const [includeUntracked, setIncludeUntracked] = useState(false);
  const [conflicts, setConflicts] = useState<string[]>([]);
  const [refreshFailed, setRefreshFailed] = useState(false);
  const { busy, issue, execute, scopedUndo } = useSourceAction(repo, onBusyChange);
  const { offer, toast } = useUndo();
  const load = useCallback(
    async (signal: AbortSignal) => {
      const next = await repoStashList(repo.path);
      if (!signal.aborted) {
        setEntries(next);
        setRefreshFailed(false);
      }
    },
    [repo.path],
  );
  useEffect(() => {
    setEntries([]);
    setConflicts([]);
    setMessage("");
    void execute((_id, signal) => load(signal));
  }, [execute, load]);
  const updated = async (signal: AbortSignal) => {
    await load(signal);
    if (!signal.aborted) onChanged?.();
  };
  const apply = (entry: StashEntry, pop: boolean) =>
    void execute(async (id, signal) => {
      const result = await repoStashApply(repo.path, entry.index, entry.commit, pop, id);
      if (!signal.aborted) setConflicts(result.conflicts);
      await updated(signal);
    });
  const drop = async (entry: StashEntry) => {
    const deleted = await execute(async (id, signal) => {
      const receipt = await repoStashDrop(repo.path, entry.index, entry.commit, id);
      await updated(signal).catch(() => {
        if (!signal.aborted) {
          setEntries([]);
          setRefreshFailed(true);
        }
      });
      return receipt;
    });
    if (!deleted) return;
    offer(
      `stash ${entry.index}를 삭제했습니다.`,
      scopedUndo(async (undoId, signal) => {
        await repoStashStore(repo.path, deleted.commit, deleted.message, undoId);
        await updated(signal);
      }),
    );
  };
  return (
    <section aria-label="Stash" className="source-stash-panel">
      <h2>Stash</h2>
      <button type="button" disabled={busy} onClick={() => void execute((_id, signal) => load(signal))}>
        stash 새로 고침
      </button>
      {issue && <p role="alert">{issue}</p>}
      {refreshFailed && <p role="alert">삭제는 완료했지만 목록을 읽지 못했습니다. 새로 고쳐 주세요.</p>}
      {conflicts.length > 0 && <p role="alert">{`충돌 파일 ${conflicts.length}개: ${conflicts.join(", ")}`}</p>}
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void execute(async (id, signal) => {
            await repoStashPush(repo.path, { message: message.trim() || null, includeUntracked }, id);
            if (!signal.aborted) {
              setMessage("");
              setConflicts([]);
            }
            await updated(signal);
          });
        }}
      >
        <label>
          stash 메시지
          <input value={message} disabled={busy} onChange={(event) => setMessage(event.target.value)} />
        </label>
        <label>
          <input
            type="checkbox"
            checked={includeUntracked}
            disabled={busy}
            onChange={(event) => setIncludeUntracked(event.target.checked)}
          />
          추적하지 않는 파일 포함
        </label>
        <button disabled={busy}>변경 임시 저장</button>
      </form>
      <ul>
        {entries.map((entry) => (
          <li key={`${entry.index}:${entry.commit}`}>
            <span>{entry.message}</span>
            <button
              type="button"
              disabled={busy}
              aria-label={`stash ${entry.index} 적용`}
              onClick={() => apply(entry, false)}
            >
              적용
            </button>
            <button
              type="button"
              disabled={busy}
              aria-label={`stash ${entry.index} 꺼내기`}
              onClick={() => apply(entry, true)}
            >
              꺼내기
            </button>
            <button type="button" disabled={busy} aria-label={`stash ${entry.index} 삭제`} onClick={() => drop(entry)}>
              삭제
            </button>
          </li>
        ))}
      </ul>
      {toast}
    </section>
  );
}
