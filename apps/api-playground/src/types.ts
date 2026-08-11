export interface KeyValue {
  key: string;
  value: string;
}

export interface AuthConfig {
  kind: string;
  username: string;
  password: string;
  token: string;
  api_key: string;
  api_value: string;
}

export interface ApiRequest {
  method: string;
  url: string;
  headers: KeyValue[];
  params: KeyValue[];
  body_kind: string;
  body: string;
  auth: AuthConfig | null;
  timeout_ms: number;
}

export interface ApiResponse {
  status: number;
  status_text: string;
  headers: KeyValue[];
  duration_ms: number;
  size_bytes: number;
  body: string;
  is_json: boolean;
}

export interface HistoryItem {
  id: string;
  saved_at: number;
  request: ApiRequest;
  status?: number;
}
