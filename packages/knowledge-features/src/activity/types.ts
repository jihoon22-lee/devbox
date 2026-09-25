export type AppTotal = import("../generated/AppTotal").AppTotal;

export type Session = import("../generated/Session").Session;

export type ProjectAssociation = import("../generated/ProjectAssociation").ProjectAssociation;

export function projectAssociationLabel(association: ProjectAssociation): string {
  return {
    mapped: "프로젝트 연결됨",
    unmapped: "등록되지 않은 프로젝트",
    unavailable: "프로젝트 연결 대기",
    ambiguous: "프로젝트 연결 중복",
    offline: "프로젝트 오프라인",
    missing: "프로젝트 없음",
    unverified: "프로젝트 연결됨 · 경로 확인 전",
  }[association.state];
}

export type ProjectCommit = import("../generated/ActivityProjectCommit").ActivityProjectCommit;

export type GitDay = import("../generated/ActivityGitDay").ActivityGitDay;

export type DaySummary = import("../generated/ActivityDaySummary").ActivityDaySummary;

export type DayPoint = import("../generated/DayPoint").DayPoint;

export type RangeSummary = import("../generated/ActivityRangeSummary").ActivityRangeSummary;
