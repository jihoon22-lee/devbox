import { invoke } from "@tauri-apps/api/core";
import { configureProductTransport, type Component } from "@devbox/knowledge-features/transport";
import { describe, makeRequest, nativeMode } from "@devbox/product-shell/api";
import { isOperation, problemMessage, type Operation } from "@devbox/product-shell/operation";
import catalog from "../../../apps/products.json";
const routeFor: Record<Component, string> = {
  "knowledge.notes": "notes", "knowledge.activity": "activity", "knowledge.search": "search",
  "knowledge.search-settings": "search", "knowledge.opener": "search", "knowledge.migration": "notes",
};
const messages: Record<string, string> = {
  tray_unavailable: "트레이를 사용할 수 없어 설정을 변경하지 않았습니다.",
  close_policy_save_failed: "종료 설정을 저장하지 못했습니다. 이전 설정을 유지했습니다.",
  target_exists: "같은 경로의 파일이 이미 있습니다. 다시 확인해서 열어 주세요.",
  vault_unavailable: "노트 저장소에 연결할 수 없습니다. 경로와 연결 상태를 확인해 주세요.",
  invalid_request: "요청을 확인해 주세요.", setup_required: "저장소를 먼저 준비해 주세요.",
  busy: "다른 작업을 마친 뒤 다시 시도해 주세요.", future_schema: "더 최신 버전의 저장소입니다. 원본을 유지했습니다.",
  store_invalid: "저장소 정보를 확인할 수 없습니다. 기존 데이터는 유지됩니다.",
  consent_save_failed: "수집 설정을 저장하지 못했습니다. 현재 수집 상태를 확인하고 다시 시도해 주세요.",
  autostart_owner_conflict: "다른 실행 파일의 시작프로그램 등록이 있어 변경하지 않았습니다.",
  provider_unavailable: "연결할 수 없습니다. 현재 내용은 유지됩니다.",
  vault_binding_unavailable: "저장소 연결을 확인할 수 없습니다. 현재 저장소를 유지했습니다.",
  restart_required: "저장소 초기화를 완료하지 못했습니다. 앱을 다시 시작해 주세요.",
  cancelled: "작업을 취소했습니다.", preview_expired: "미리보기가 만료되었습니다. 새로 준비해 주세요.",
  preview_stale: "미리보기가 오래되었습니다. 다시 확인해 주세요.",
};
configureProductTransport(async <T>(component: Component, method: string, args: Record<string, unknown>): Promise<T> => {
  if (!nativeMode) throw new Error("데스크톱 앱에서 사용할 수 있습니다.");
  const description = await describe("knowledge");
  const header = makeRequest(description.handshake, routeFor[component], Date.now(), description.context);
  const provenance = { product: "knowledge", component, requestId: header.requestId, revision: catalog.catalogRevision };
  let response: { operation: Operation; value: T };
  try { response = await invoke("plugin:knowledge|execute", { request: { header, component, method, args } }); }
  catch (problem) { throw new Error(problemMessage(problem, provenance)); }
  if (!response || !isOperation(response.operation, provenance)) throw new Error("작업 응답의 출처를 확인할 수 없습니다.");
  if (response.operation.outcome.state !== "succeeded") {
    const value = response.value as { issue?: unknown } | null;
    const message = value && typeof value.issue === "string" ? messages[value.issue] : undefined;
    const error = new Error(message ?? "작업을 완료하지 못했습니다. 입력과 저장 위치를 확인해 주세요.");
    if (value && typeof value.issue === "string" && messages[value.issue]) error.name = value.issue;
    throw error;
  }
  return response.value;
});
