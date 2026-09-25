import type { NoteJournalEntry } from "../api";
export default function RecoveryBanner({
  entries,
  otherVaultCount,
  busy,
  onOpen,
  onDiscard,
  onDiscardOther,
}: {
  entries: NoteJournalEntry[];
  otherVaultCount: number;
  busy: boolean;
  onOpen: (entry: NoteJournalEntry) => void;
  onDiscard: (entry: NoteJournalEntry) => void;
  onDiscardOther: () => void;
}) {
  if (!entries.length && !otherVaultCount) return null;
  return (
    <section className="recovery-banner" role="region" aria-label="저장하지 않은 노트 복구">
      {otherVaultCount > 0 && (
        <div>
          <p>
            다른 노트 폴더에서 저장하지 않은 복구본 {otherVaultCount}개가 있습니다. 그 폴더를 다시 열면 복구할 수
            있습니다.
          </p>
          <button disabled={busy} onClick={onDiscardOther}>
            다른 폴더 복구본 버리기
          </button>
        </div>
      )}
      {entries.length > 0 && (
        <>
          <p>저장하지 않은 편집 {entries.length}개가 남아 있습니다.</p>
          <ul>
            {entries.map((entry) => (
              <li key={entry.path}>
                <span>{entry.path}</span>{" "}
                <time dateTime={new Date(entry.savedAtMs).toISOString()}>
                  {new Date(entry.savedAtMs).toLocaleString("ko-KR")}
                </time>{" "}
                <button disabled={busy} onClick={() => onOpen(entry)} aria-label={`${entry.path} 열어서 확인`}>
                  열어서 확인
                </button>{" "}
                <button disabled={busy} onClick={() => onDiscard(entry)} aria-label={`${entry.path} 복구본 버리기`}>
                  버리기
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}
