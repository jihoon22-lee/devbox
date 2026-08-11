export interface AppTotal {
  app: string;
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
