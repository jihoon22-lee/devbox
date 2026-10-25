/** A completed response is exported by its daemon history identity, never by renderer body text. */
import { useEffect, useRef, useState } from "react";
import type { AppendResponseNote, NoteTree, ResponseNoteTarget } from "../../../rpc/gen/rpc";
import { messageFor } from "../../../rpc/messages";
import { Button, Dialog } from "../../../ui";
import s from "./Api.module.css";
import { SendFailure, useApiBackend } from "./backend";

export function ResponseNoteAction({ historyId, ownerKey }: { historyId: string | null; ownerKey: string }) {
  return historyId ? <Action key={`${ownerKey}:${historyId}`} historyId={historyId} /> : null;
}

function Action({ historyId }: { historyId: string }) {
  const backend = useApiBackend();
  const [open, setOpen] = useState(false);
  const [kind, setKind] = useState<"daily" | "existing">("daily");
  const [path, setPath] = useState("");
  const [tree, setTree] = useState<NoteTree | null>(null);
  const [loading, setLoading] = useState(false);
  const [pending, setPending] = useState(false);
  const [intent, setIntent] = useState<AppendResponseNote | null>(null);
  const [error, setError] = useState<{ code: string; detail?: unknown } | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const generation = useRef(0);
  const busy = useRef(false);
  useEffect(
    () => () => {
      generation.current += 1;
    },
    [],
  );
  const fail = (e: unknown) =>
    setError(e instanceof SendFailure ? { code: e.code, detail: e.detail } : { code: "internal" });
  async function load(cursor: string | null) {
    const at = generation.current;
    setLoading(true);
    try {
      const page = await backend.noteTargets(cursor);
      if (at !== generation.current) return;
      setTree((before) => (cursor && before ? { ...page, items: [...before.items, ...page.items] } : page));
    } catch (e) {
      if (at === generation.current) fail(e);
    } finally {
      if (at === generation.current) setLoading(false);
    }
  }
  async function append() {
    if (busy.current || (kind === "existing" && !path && !intent)) return;
    busy.current = true;
    setPending(true);
    setError(null);
    const at = generation.current;
    try {
      let request = intent;
      if (!request) {
        const target: ResponseNoteTarget = kind === "daily" ? { kind: "daily" } : { kind: "existing", path };
        const selected = await backend.selectResponseTarget(target);
        if (at !== generation.current) return;
        request = { historyId, selection: selected.selection, operationId: crypto.randomUUID().replaceAll("-", "") };
        setIntent(request);
      }
      const result = await backend.appendResponse(request);
      if (at !== generation.current) return;
      // Success is the mutation result. A later list refresh is never grounds for another append.
      setSaved(result.path);
      setOpen(false);
      setIntent(null);
    } catch (e) {
      if (at === generation.current) fail(e);
    } finally {
      if (at === generation.current) {
        busy.current = false;
        setPending(false);
      }
    }
  }
  const message = error ? messageFor(error.code as never, error.detail) : null;
  const canReselect =
    error && ["notes.response_target_changed", "notes.too_large", "notes.invalid_input"].includes(error.code);
  return (
    <>
      <div className={s.bar}>
        <Button size="sm" onClick={() => setOpen(true)} disabled={pending}>
          노트에 붙이기
        </Button>
        {saved && <span role="status">{saved}에 붙였습니다.</span>}
      </div>
      <Dialog
        open={open}
        onOpenChange={(value) => {
          if (!pending) setOpen(value);
        }}
        title="응답을 노트에 붙이기"
        footer={
          <>
            <Button variant="ghost" disabled={pending} onClick={() => setOpen(false)}>
              닫기
            </Button>
            {canReselect && (
              <Button
                variant="ghost"
                onClick={() => {
                  setIntent(null);
                  setError(null);
                }}
              >
                대상 다시 선택
              </Button>
            )}
            <Button disabled={pending || (kind === "existing" && !path && !intent)} onClick={() => void append()}>
              {pending ? "붙이는 중…" : intent ? "같은 작업 다시 확인" : "붙이기"}
            </Button>
          </>
        }
      >
        <p>완료된 원래 응답의 가려진 기록을 붙입니다. 본문은 최대 1 MiB이며 이진 응답은 정보만 붙입니다.</p>
        <label>
          붙일 곳
          <select
            aria-label="붙일 곳"
            value={kind}
            disabled={pending || !!intent}
            onChange={(e) => {
              const value = e.target.value as "daily" | "existing";
              setKind(value);
              if (value === "existing" && !tree) void load(null);
            }}
          >
            <option value="daily">일일 기록</option>
            <option value="existing">고른 노트</option>
          </select>
        </label>
        {kind === "existing" && (
          <>
            <label>
              노트
              <select
                aria-label="노트"
                value={path}
                disabled={loading || pending || !!intent}
                onChange={(e) => setPath(e.target.value)}
              >
                <option value="">노트를 고르세요</option>
                {tree?.items
                  .filter((item) => !item.dir)
                  .map((item) => (
                    <option key={item.path} value={item.path}>
                      {item.title} · {item.path}
                    </option>
                  ))}
              </select>
            </label>
            {loading && <p role="status">노트 목록을 읽는 중…</p>}
            {tree?.nextCursor && (
              <Button
                variant="ghost"
                disabled={loading || pending || !!intent}
                onClick={() => void load(tree.nextCursor)}
              >
                더 불러오기
              </Button>
            )}
            {tree?.truncated && !tree.nextCursor && <p>목록이 잘렸습니다. 노트 목록을 정리한 뒤 다시 선택해 주세요.</p>}
          </>
        )}
        {message && (
          <div role="alert">
            <strong>{message.title}</strong>
            <p>{message.action}</p>
          </div>
        )}
      </Dialog>
    </>
  );
}
