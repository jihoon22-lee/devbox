import { useMemo, useState } from "react";
import type { DocKey } from "../../rpc/gen/rpc";
import { Banner, Button, DiffView, TextArea } from "../../ui";
import type { Doc } from "../documents/machine";
import { keyId } from "../documents/offlineJournal";
import { useDocuments } from "../documents/store";
import { lineDiff } from "./lineDiff";
import s from "./notes.module.css";

/**
 * Conflict: the disk changed under unsaved edits. Shows disk → buffer and the three choices of
 * 01-design §8.4. [디스크 것] drops both journals so the draft never returns (AR-F13).
 */
export function CompareView({ docKey }: { docKey: DocKey }) {
  const doc = useDocuments((st) => Object.values(st.docs).find((d) => keyId(d.key) === keyId(docKey)));
  if (!doc || doc.state !== "Conflict") return null;
  return <Conflict key={`${doc.id}:${JSON.stringify(doc.conflictRev)}`} doc={doc} />;
}

function Conflict({ doc }: { doc: Doc }) {
  const docKey = doc.key;
  const { resolve } = useDocuments.getState();
  const [merging, setMerging] = useState<string | null>(null);
  const disk = doc?.disk;
  const diff = useMemo(
    () => (doc && disk?.text != null ? lineDiff(disk.text, doc.text) : null),
    [doc?.text, disk?.text, doc],
  );
  if (!doc || doc.state !== "Conflict") return null;
  const deleted = disk && disk.rev === null;
  return (
    <section className={s.panel} aria-label="충돌 비교">
      <Banner tone="warn">
        {deleted
          ? "편집하는 동안 디스크의 파일이 지워졌습니다. 내 것을 유지하면 다시 만듭니다."
          : "편집하는 동안 다른 곳에서 이 문서를 고쳤습니다. 덮어쓰지 않았습니다. 어느 쪽을 남길지 골라 주세요."}
      </Banner>
      {merging === null ? (
        <>
          {diff ? (
            <DiffView file={diff} maxLines={2000} />
          ) : (
            !deleted && <p role="status">디스크 내용을 읽는 중입니다…</p>
          )}
          <div className={s.actions}>
            <Button variant="primary" onClick={() => resolve(docKey, "mine")}>
              내 것 유지
            </Button>
            <Button disabled={disk?.text == null} onClick={() => resolve(docKey, "disk")}>
              디스크 것
            </Button>
            <Button onClick={() => setMerging(doc.text)}>직접 병합</Button>
          </div>
        </>
      ) : (
        <>
          <TextArea
            aria-label="병합한 내용"
            className={s.merge}
            value={merging}
            onChange={(event) => setMerging(event.currentTarget.value)}
          />
          <div className={s.actions}>
            <Button
              variant="primary"
              onClick={() => {
                resolve(docKey, "merged", merging);
                setMerging(null);
              }}
            >
              병합한 내용 저장
            </Button>
            <Button onClick={() => setMerging(null)}>취소</Button>
          </div>
        </>
      )}
    </section>
  );
}
