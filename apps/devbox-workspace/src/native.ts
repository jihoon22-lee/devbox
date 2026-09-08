import { invoke } from "@tauri-apps/api/core";
import { describe, makeRequest } from "@devbox/product-shell/api";
import { isOperation } from "@devbox/product-shell/operation";
import catalog from "../../products.json";

const issues: Record<string, string> = {
  initializing: "저장된 정보를 불러오고 있습니다.",
  busy: "앞선 작업이 끝난 뒤 다시 시도해 주세요.",
  store_owner_busy: "다른 작업에서 저장소를 사용하고 있습니다.",
  store_pointer_changed: "저장소가 변경되었습니다. 앱을 다시 시작해 주세요.",
  invalid_registry: "프로젝트 목록을 읽지 못했습니다. 기존 파일을 확인해 주세요.",
  invalid_files_store: "편집기 설정이나 복구 파일을 읽지 못했습니다. 기존 파일을 확인해 주세요.",
  invalid_overview_store: "프로젝트 설정을 읽지 못했습니다. 기존 파일을 확인해 주세요.",
  project_probe_timeout: "폴더 확인 시간이 초과되었습니다. 연결 상태를 확인해 주세요.",
  project_object_unavailable: "폴더 또는 Git 정보를 읽지 못했습니다.",
  unsafe_project_object: "연결된 경로는 등록할 수 없습니다. 실제 폴더를 선택해 주세요.",
  project_object_changed: "확인한 폴더가 변경되었습니다. 다시 확인해 주세요.",
  project_preview_stale: "등록 확인이 만료되었습니다. 폴더를 다시 확인해 주세요.",
  stale_registry: "프로젝트 목록이 변경되었습니다. 새로 고친 뒤 다시 시도해 주세요.",
  stale_context: "프로젝트 연결이 변경되었습니다. 작업 폴더를 다시 선택해 주세요.",
  project_binding_changed: "등록한 폴더가 교체되었습니다. 폴더를 확인하고 경로를 다시 연결해 주세요.",
  context_selection_expired: "프로젝트 확인 시간이 초과되었습니다. 다시 선택해 주세요.",
  project_has_legacy_references: "이 프로젝트를 참조하는 항목이 있어 등록을 해제할 수 없습니다.",
  wsl_admission_required: "WSL 프로젝트 연결은 아직 사용할 수 없습니다.",
};
export function issueMessage(issue: string): string { return issues[issue] ?? "작업을 완료하지 못했습니다. 상태를 확인하고 다시 시도해 주세요."; }
export async function nativeCall<T>(component: string, method: string, args: Record<string, unknown> = {}, route = "overview"): Promise<T> {
  const description = await describe("workspace");
  const header = makeRequest(description.handshake, route, Date.now(), description.context);
  const provenance = { product: "workspace", component, requestId: header.requestId, revision: catalog.catalogRevision };
  const response = await invoke<{operation: unknown; value: T & {issue?: string}}>("plugin:workspace|execute", {request:{header, component, method, args}});
  if (!response || !isOperation(response.operation, provenance)) throw new Error("응답을 확인하지 못했습니다.");
  if (response.operation.outcome.state !== "succeeded") throw new Error(issueMessage(response.value?.issue ?? "operation_failed"));
  return response.value;
}
