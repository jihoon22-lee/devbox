import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";
import type { ApiRequest, ApiResponse } from "./types";

/** HTTP 요청 전송. 브라우저 미리보기에서는 fetch(CORS 제약 존재)로 대체한다. */
export async function sendRequest(req: ApiRequest): Promise<ApiResponse> {
  if (!isTauri()) return browserFetch(req);
  return invoke<ApiResponse>("send_request", { req });
}

async function browserFetch(req: ApiRequest): Promise<ApiResponse> {
  const start = performance.now();
  const headers: Record<string, string> = {};
  for (const h of req.headers) if (h.key) headers[h.key] = h.value;
  if (req.auth?.kind === "basic") {
    headers["Authorization"] = "Basic " + btoa(`${req.auth.username}:${req.auth.password}`);
  } else if (req.auth?.kind === "bearer") {
    headers["Authorization"] = "Bearer " + req.auth.token;
  } else if (req.auth?.kind === "apikey") {
    headers[req.auth.api_key] = req.auth.api_value;
  }
  const params = new URLSearchParams();
  for (const p of req.params) if (p.key) params.append(p.key, p.value);
  const sep = req.url.includes("?") ? "&" : "?";
  const url = params.size ? req.url + sep + params.toString() : req.url;

  let body: string | undefined;
  if (req.body_kind === "json" && req.body.trim()) {
    headers["Content-Type"] = "application/json";
    body = req.body;
  } else if (req.body_kind === "raw" && req.body) {
    body = req.body;
  }

  const resp = await fetch(url, { method: req.method, headers, body });
  const duration_ms = Math.round(performance.now() - start);
  const text = await resp.text();
  const respHeaders: { key: string; value: string }[] = [];
  resp.headers.forEach((v, k) => respHeaders.push({ key: k, value: v }));

  return {
    status: resp.status,
    status_text: resp.statusText,
    headers: respHeaders,
    duration_ms,
    size_bytes: text.length,
    body: text,
    is_json: (resp.headers.get("content-type") ?? "").includes("json"),
  };
}
