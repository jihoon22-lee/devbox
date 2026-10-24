import { useMemo } from "react";
import type { DocKey } from "../../rpc/gen/rpc";
import { Banner, Button, DiffView } from "../../ui";
import { keyId } from "../documents/offlineJournal";
import { useDocuments } from "../documents/store";
import { lineDiff } from "./lineDiff";
import s from "./notes.module.css";

/**
 * Recovering: a saved draft (this device or another window) differs from the disk.
 * [복원] puts the draft in the buffer, [버림] drops it. With unsaved edits here as well,
 * [버림] keeps this buffer and only [모두 버리고 닫기] closes after asking to close.
 */
export function RecoveryView({ docKey }: { docKey: DocKey }) {
  const doc = useDocuments((st) => Object.values(st.docs).find((d) => keyId(d.key) === keyId(docKey)));
  const store = useDocuments.getState();
  const draft = doc?.recovery?.text;
  const diff = useMemo(
    () =>
      doc && draft !== undefined
        ? lineDiff(doc.text, draft, { old: doc.editSeq > doc.savedSeq ? "이 창의 편집" : "디스크", new: "초안" })
        : null,
    [doc, draft],
  );
  if (!doc || doc.state !== "Recovering" || !doc.recovery) return null;
  const mine = doc.editSeq > doc.savedSeq;
  const other = doc.recovery.other;
  const dispatch = (event: Parameters<typeof store.dispatch>[1]) => store.dispatch(doc.id, event);
  return (
    <section className={s.panel} aria-label="저장하지 않은 초안 복구">
      <Banner tone="warn">
        {mine
          ? "다른 창이 남긴 초안이 있습니다. 이 창의 편집도 아직 저장되지 않았습니다."
          : "저장되지 않은 초안이 남아 있습니다. 복원하거나 버릴 수 있습니다."}
      </Banner>
      {diff ? <DiffView file={diff} maxLines={2000} /> : <p role="status">초안을 읽는 중입니다…</p>}
      <div className={s.actions}>
        <Button variant="primary" disabled={draft === undefined} onClick={() => dispatch({ type: "restore" })}>
          {mine ? "다른 창 초안 쓰기" : "복원"}
        </Button>
        {other && (
          <Button disabled={other.text === undefined} onClick={() => dispatch({ type: "restore", which: "other" })}>
            다른 초안 복원
          </Button>
        )}
        <Button onClick={() => dispatch({ type: "discard" })}>{mine ? "내 것 유지" : "버림"}</Button>
        {doc.closeRequested && mine && (
          <Button variant="danger" onClick={() => store.close(docKey, "discardAll")}>
            모두 버리고 닫기
          </Button>
        )}
      </div>
    </section>
  );
}
