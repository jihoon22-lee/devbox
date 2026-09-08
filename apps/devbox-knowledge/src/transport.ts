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
  search_stale: "검색 결과가 오래되었거나 파일 연결이 바뀌었습니다. 다시 검색해 주세요.",
  import_invalid: "가져오기 데이터의 내용이나 형식을 확인하지 못했습니다. 원본과 현재 데이터를 유지했습니다.",
  import_schema_unsupported: "이전 데이터의 형식을 지원하지 않습니다. 원본과 현재 저장소를 유지했습니다.",
  import_source_changed: "미리보기 이후 이전 앱의 데이터가 바뀌었습니다. 다시 준비해 주세요.",
  import_restart_required: "현재 노트를 저장하고 앱을 다시 시작한 뒤 가져오기를 진행해 주세요.",
  import_limit_exceeded: "가져오기 크기나 항목 수의 한도를 넘었습니다. 원본을 유지했습니다. 완료하지 않은 준비 작업도 확인해 주세요.",
  import_disk_full: "가져오기 사본과 복구 데이터를 보관할 여유 공간이 부족합니다.",
  import_timed_out: "가져오기가 제한 시간을 넘었습니다. 이전 앱을 닫고 다시 준비해 주세요.",
  legacy_writer_active: "이전 Knowledge 앱을 완전히 종료한 뒤 다시 적용해 주세요. 같은 노트를 동시에 수정할 수 없습니다.",
  vault_owner_busy: "다른 Knowledge 설치가 이 노트 저장소를 사용 중입니다. 해당 앱을 종료한 뒤 다시 시도해 주세요.",
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
export function issueError(issue: unknown): Error {
  const known = typeof issue === "string" && Object.prototype.hasOwnProperty.call(messages, issue);
  const error = new Error(known ? messages[issue] : "작업을 완료하지 못했습니다. 입력과 저장 위치를 확인해 주세요.");
  if (known) error.name = issue;
  return error;
}
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
    throw issueError(value?.issue);
  }
  return response.value;
});
