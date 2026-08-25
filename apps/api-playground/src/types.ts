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

/** 사용자가 편집하고 저장하는 요청 원본. 환경 변수 참조는 해석하지 않은 채 유지한다. */
export interface RequestTemplate {
  method: string;
  url: string;
  headers: KeyValue[];
  params: KeyValue[];
  body_kind: string;
  body: string;
  auth: AuthConfig | null;
  timeout_ms: number;
}

/**
 * 저장 직전에 민감한 직접 입력값을 제거한 요청.
 * 실제 전송에 쓰이는 ResolvedRequest는 Rust 내부에만 존재한다.
 */
export interface PersistedHistoryRequest extends RequestTemplate {
  requiresSecretReview: boolean;
}

export interface ApiResponse {
  status: number;
  status_text: string;
  headers: KeyValue[];
  duration_ms: number;
  size_bytes: number;
  body: string;
  is_json: boolean;
  final_url: string;
  redirects: RedirectHop[];
}

export interface RedirectHop {
  status: number;
  location: string;
}

export interface HistoryItem {
  id: string;
  /** 사용자가 지정한 표시 이름. 기존 v2 항목은 URL을 fallback으로 사용한다. */
  name?: string;
  saved_at: number;
  request: PersistedHistoryRequest;
  status?: number;
}
