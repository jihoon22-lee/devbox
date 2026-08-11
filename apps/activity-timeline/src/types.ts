export interface Session {
  id: number;
  app: string;
  title: string;
  start_ts: number;
  end_ts: number;
  duration_ms: number;
}

export interface AppTotal {
  app: string;
  duration_ms: number;
  sessions: number;
}
