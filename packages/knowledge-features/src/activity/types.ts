export interface AppTotal {
  app: string;
  duration_ms: number;
  sessions: number;
}

export interface Session {
  id: number;
  app: string;
  title: string;
  start_ts: number;
  end_ts: number;
  duration_ms: number;
}

export interface ProjectAssociation {
  state: "mapped" | "unmapped" | "unavailable" | "ambiguous" | "offline" | "missing";
  context?: { projectId: string; worktreeId: string };
}

export function projectAssociationLabel(association: ProjectAssociation): string {
  return { mapped: "프로젝트 연결됨", unmapped: "등록되지 않은 프로젝트", unavailable: "프로젝트 연결 대기", ambiguous: "프로젝트 연결 중복", offline: "프로젝트 오프라인", missing: "프로젝트 없음" }[association.state];
}

export interface ProjectCommit {
  path: string;
  commits: number;
  error_code?: string | null;
  projectAssociation?: ProjectAssociation;
}

export interface GitDay {
  projects: ProjectCommit[];
  total_commits: number;
}

export interface DaySummary {
  date: string;
  pc_usage_ms: number;
  app_totals: AppTotal[];
  git: GitDay;
}

export interface DayPoint {
  day_ms: number;
  pc_usage_ms: number;
}

export interface RangeSummary {
  label: string;
  pc_usage_ms: number;
  app_totals: AppTotal[];
  git: GitDay;
  daily: DayPoint[];
}
