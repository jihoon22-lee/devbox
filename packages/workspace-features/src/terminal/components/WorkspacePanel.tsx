import type { MultiplexerAvailability, WorkspaceProfile } from "../types";

interface WorkspacePanelProps {
  profiles: readonly WorkspaceProfile[];
  muxAvailability: readonly MultiplexerAvailability[];
  busy: boolean;
  onSaveCurrent: () => void;
  onOpen: (profile: WorkspaceProfile) => void;
  onDelete: (profile: WorkspaceProfile) => void;
}

const SOURCE_LABELS: Record<NonNullable<MultiplexerAvailability["source"]>, string> = {
  path: "배포판 PATH",
  userLocal: "사용자 로컬",
  cargoBin: "Cargo bin",
  system: "시스템",
};

function availabilityTitle(item: MultiplexerAvailability): string {
  if (item.status === "error") return "설치 여부를 확인할 수 없습니다";
  if (item.status === "missing") return "설치되지 않음";
  const details = [item.version, item.source ? SOURCE_LABELS[item.source] : null].filter(Boolean);
  return details.join(" · ") || "사용 가능";
}

export default function WorkspacePanel({
  profiles,
  muxAvailability,
  busy,
  onSaveCurrent,
  onOpen,
  onDelete,
}: WorkspacePanelProps) {
  return (
    <section className="workspace-panel" aria-label="터미널 프로필">
      <div className="section-head">
        <strong>터미널 프로필</strong>
        <button type="button" className="btn compact" disabled={busy} onClick={onSaveCurrent}>현재 상태 저장</button>
      </div>
      <div className="profile-list">
        {profiles.map((profile) => (
          <div className="profile-row" key={profile.id}>
            <button
              type="button"
              className="profile-open"
              disabled={busy}
              title={`${profile.tabs.length}개 탭 · ${profile.panes.length}개 팬`}
              onClick={() => onOpen(profile)}
            >
              <span>{profile.name}</span>
              <small>{profile.tabs.length}탭 · {profile.panes.length}팬</small>
            </button>
            <button
              type="button"
              className="profile-delete"
              disabled={busy}
              aria-label={`${profile.name} 프로필 삭제`}
              onClick={() => onDelete(profile)}
            >✕</button>
          </div>
        ))}
        {profiles.length === 0 && <div className="dim">저장한 프로필이 없습니다.</div>}
      </div>
      <div className="mux-status" aria-label="멀티플렉서 상태">
        {muxAvailability.filter((item) => item.kind !== "native").map((item) => (
          <span key={item.kind} className={item.status} title={availabilityTitle(item)}>
            {item.kind}: {item.status === "available"
              ? "사용 가능"
              : item.status === "missing"
                ? "없음 · native 사용"
                : "확인 오류 · native 사용"}
          </span>
        ))}
      </div>
    </section>
  );
}
