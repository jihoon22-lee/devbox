import { confirmAction } from "@devbox/product-shell/confirm";
import { useCallback, useEffect, useRef, useState, useSyncExternalStore, type RefObject } from "react";
import ChangeSetPreview from "@devbox/diff-view";
import {
  clearNoteJournal,
  readFile,
  writeFile,
  createFile,
  discardOtherVaultJournal,
  loadNoteJournal,
  type NoteJournalEntry,
  type NoteJournalView,
} from "../api";
import type { NoteDocument, NoteView } from "../noteDocument";
import type { NoteAutosave } from "../autosave";
import type { NoteJournal } from "../journal";
import RecoveryBanner from "./RecoveryBanner";

type Pending = { entry: NoteJournalEntry; disk: string; path: string; sourceVersion: number; revision: string };
type Confirmation = { kind: "entry"; entry: NoteJournalEntry } | { kind: "other"; count: number };
const empty: NoteJournalView = { entries: [], otherVaultCount: 0 };
const sameEntry = (a: NoteJournalEntry, b: NoteJournalEntry) =>
  a.path === b.path && a.content === b.content && a.baseRevision === b.baseRevision && a.savedAtMs === b.savedAtMs;
const matches = (pending: Pending, view: NoteView) =>
  !view.saving &&
  pending.path === view.path &&
  pending.sourceVersion === view.sourceVersion &&
  pending.revision === view.revision;

/** Recovery is mounted after native activation, independently of metadata refresh. */
export default function RecoveryControls({
  document,
  autosave,
  journal,
  onBusy,
}: {
  document: NoteDocument;
  autosave: RefObject<NoteAutosave | null>;
  journal: RefObject<NoteJournal | null>;
  onBusy: (busy: boolean) => void;
}) {
  const note = useSyncExternalStore(document.subscribe, document.snapshot);
  const [view, setView] = useState<NoteJournalView>(empty);
  const [preview, setPreview] = useState<NoteJournalEntry | null>(null);
  const [missingRevision, setMissingRevision] = useState<string | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [confirmation, setConfirmation] = useState<Confirmation | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false),
    alive = useRef(false),
    generation = useRef(0);
  const changeBusy = useCallback(
    (value: boolean) => {
      busyRef.current = value;
      setBusy(value);
      onBusy(value);
    },
    [onBusy],
  );
  const current = (request: number) => alive.current && request === generation.current;
  const reload = useCallback(async () => {
    const request = ++generation.current;
    try {
      const value = await loadNoteJournal();
      if (alive.current && request === generation.current) {
        setView(value);
        setError("");
      }
    } catch {
      if (alive.current && request === generation.current) setError("복구 정보를 읽지 못했습니다.");
    }
  }, []);
  // biome-ignore lint/correctness/useExhaustiveDependencies: existing dependency list; review in P1-15
  useEffect(() => {
    alive.current = true;
    setView(empty);
    setPending(null);
    setConfirmation(null);
    void reload();
    return () => {
      alive.current = false;
      generation.current++;
      onBusy(false);
    };
  }, [document, reload, onBusy]);
  useEffect(() => {
    if (pending && !matches(pending, note)) setPending(null);
  }, [pending, note]);

  const open = async (entry: NoteJournalEntry) => {
    if (busyRef.current || !view.entries.some((value) => sameEntry(value, entry))) return;
    const request = ++generation.current;
    changeBusy(true);
    setError("");
    setPending(null);
    setPreview(entry);
    setMissingRevision(null);
    setConfirmation(null);
    try {
      await journal.current?.settled();
      if (!current(request)) return;
      const before = document.snapshot();
      const disk = await readFile(entry.path);
      if (!current(request) || document.snapshot().sourceVersion !== before.sourceVersion) return;
      if (disk.content === null) {
        setMissingRevision(disk.revision);
        return;
      }
      const opened = await document.open(
        async () => ({ ...disk, path: entry.path, content: disk.content! }),
        () => confirmAction("저장하지 않은 변경사항이 있습니다. 계속할까요?"),
      );
      if (!current(request) || !opened) return;
      const after = document.snapshot();
      if (after.path !== entry.path || after.sourceVersion !== before.sourceVersion + 1 || after.saving) return;
      if (after.revision === entry.baseRevision) {
        document.edit(entry.content);
        journal.current?.adoptRestored(entry.path);
      } else
        setPending({
          entry,
          disk: after.content,
          path: entry.path,
          sourceVersion: after.sourceVersion,
          revision: after.revision,
        });
    } catch {
      if (current(request))
        setError(
          "원본에 연결할 수 없습니다. 복구본 본문을 확인하거나 복사할 수 있으며 복원은 연결 후 다시 시도해 주세요.",
        );
    } finally {
      if (current(request)) changeBusy(false);
    }
  };
  const apply = () => {
    if (!pending || busyRef.current) return;
    if (!matches(pending, document.snapshot())) {
      setPending(null);
      return;
    }
    autosave.current?.pause();
    document.edit(pending.entry.content);
    journal.current?.adoptRestored(pending.path);

    setPending(null);
  };
  const restore = async (anotherName: boolean) => {
    if (!preview || busyRef.current) return;
    const entry = preview;
    const target = anotherName ? prompt("복원할 새 노트 이름 (.md)", entry.path)?.trim() : entry.path;
    if (!target || (!anotherName && !missingRevision)) return;
    const decisionGeneration = generation.current;
    const source = document.snapshot().sourceVersion;
    if (!(await confirmAction(`${target}에 복구본을 복원할까요? 기존 파일은 덮어쓰지 않습니다.`))) return;
    if (busyRef.current || generation.current !== decisionGeneration || document.snapshot().sourceVersion !== source)
      return;
    const request = ++generation.current;
    changeBusy(true);
    setError("");
    try {
      await journal.current?.settled();
      const latest = await loadNoteJournal();
      if (!current(request) || !latest.entries.some((value) => sameEntry(value, entry))) throw new Error("stale");
      if (anotherName) await createFile(target, entry.content);
      else {
        const saved = await writeFile(target, entry.content, missingRevision!);
        if (saved.saveOutcome && saved.saveOutcome.state !== "applied") throw new Error("unconfirmed");
      }
      await clearNoteJournal(entry.path);
      if (!current(request)) return;
      setPreview(null);
      setMissingRevision(null);
      await reload();
      if (document.snapshot().sourceVersion === source && !document.unsaved())
        await document.openPath(target, () => true);
    } catch {
      if (current(request)) setError("복원을 완료하지 못했습니다. 원본과 복구본을 확인한 뒤 다시 시도해 주세요.");
    } finally {
      if (alive.current) changeBusy(false);
    }
  };
  const discard = async () => {
    if (!confirmation || busyRef.current) return;
    const selected = confirmation,
      request = ++generation.current;
    let deleted = false;
    changeBusy(true);
    setError("");
    try {
      await journal.current?.settled();
      if (!current(request)) return;
      const latest = await loadNoteJournal();
      if (!current(request)) return;
      const unchanged =
        selected.kind === "other"
          ? latest.otherVaultCount === selected.count
          : latest.entries.some((entry) => sameEntry(entry, selected.entry));
      if (!unchanged) {
        setView(latest);
        setConfirmation(null);
        setError("복구 목록이 바뀌었습니다. 다시 확인해 주세요.");
        return;
      }
      if (selected.kind === "other") await discardOtherVaultJournal();
      else await clearNoteJournal(selected.entry.path);
      deleted = true;
      if (!current(request)) return;
      setConfirmation(null);
      if (selected.kind === "entry")
        setPending((previous) => (previous?.path === selected.entry.path ? null : previous));
      setView(
        selected.kind === "other"
          ? { ...latest, otherVaultCount: 0 }
          : { ...latest, entries: latest.entries.filter((entry) => entry.path !== selected.entry.path) },
      );
      const refreshed = await loadNoteJournal();
      if (current(request)) setView(refreshed);
    } catch {
      if (current(request))
        setError(
          deleted
            ? "복구본은 버렸지만 목록을 다시 읽지 못했습니다."
            : "복구본을 버리지 못했습니다. 복구본은 유지됩니다.",
        );
    } finally {
      if (current(request)) changeBusy(false);
    }
  };
  return (
    <div className="note-recovery-controls">
      <RecoveryBanner
        entries={view.entries}
        otherVaultCount={view.otherVaultCount}
        busy={busy}
        onOpen={(entry) => void open(entry)}
        onDiscard={(entry) => setConfirmation({ kind: "entry", entry })}
        onDiscardOther={() => setConfirmation({ kind: "other", count: view.otherVaultCount })}
      />
      {preview && (
        <section role="region" aria-label="복구본 본문">
          <p>{preview.path} — 저장된 복구본입니다. 복사나 편집 적용만으로 복구본을 삭제하지 않습니다.</p>
          <pre>{preview.content}</pre>
          <button
            disabled={busy}
            onClick={() => {
              void navigator.clipboard
                .writeText(preview.content)
                .catch(() => setError("복구본을 복사하지 못했습니다."));
            }}
          >
            복구본 복사
          </button>
          {missingRevision && (
            <button disabled={busy} onClick={() => void restore(false)}>
              삭제된 노트 재생성
            </button>
          )}
          {missingRevision && (
            <button disabled={busy} onClick={() => void restore(true)}>
              다른 이름으로 복원
            </button>
          )}
          <button disabled={busy} onClick={() => setPreview(null)}>
            복구본 본문 닫기
          </button>
        </section>
      )}
      {confirmation && (
        <section role="region" aria-label="복구본 삭제 확인">
          <p>
            {confirmation.kind === "other"
              ? `다른 노트 폴더의 복구본 ${confirmation.count}개를 영구 삭제합니다. 이 작업은 되돌릴 수 없습니다.`
              : `${confirmation.entry.path} 복구본을 영구 삭제합니다. 이 작업은 되돌릴 수 없습니다.`}
          </p>
          <button disabled={busy} onClick={() => void discard()}>
            복구본 삭제 확인
          </button>
          <button disabled={busy} onClick={() => setConfirmation(null)}>
            복구본 삭제 취소
          </button>
        </section>
      )}
      {pending && matches(pending, note) && (
        <section role="region" aria-label="복구본 비교">
          <p>
            복구본을 저장한 뒤 디스크의 노트가 바뀌었습니다. 복구본을 적용하면 직접 저장할 때까지 자동 저장을 멈춥니다.
          </p>
          <ChangeSetPreview
            selectable={false}
            disabled={busy}
            approveLabel="복구본으로 바꾸기"
            onApprove={apply}
            items={[
              { path: pending.path, before: pending.disk, after: pending.entry.content, meta: "디스크 내용 → 복구본" },
            ]}
          />
          <button disabled={busy} onClick={() => setConfirmation({ kind: "entry", entry: pending.entry })}>
            디스크 내용 유지
          </button>
        </section>
      )}
      {busy && <p role="status">복구 정보를 처리하고 있습니다…</p>}
      {error && (
        <div>
          <p role="alert">{error}</p>
          <button disabled={busy} onClick={() => void reload()}>
            복구 목록 다시 읽기
          </button>
        </div>
      )}
    </div>
  );
}
