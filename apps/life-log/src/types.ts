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

export interface ProjectCommit {
  path: string;
  commits: number;
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
