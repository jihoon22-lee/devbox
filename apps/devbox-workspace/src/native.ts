import { invoke } from "@tauri-apps/api/core";
import { describe, makeRequest, type Description } from "@devbox/product-shell/api";
import { isOperation, problemMessage } from "@devbox/product-shell/operation";
import { WorkspaceOperationError } from "@devbox/workspace-features/transport";
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
  file_selection_required: "파일 선택 버튼에서 열 파일을 선택해 주세요.",
  project_selection_required: "개요에서 작업할 프로젝트를 선택해 주세요.",
  file_context_changed: "파일을 연 프로젝트가 변경되었습니다. 해당 프로젝트를 다시 선택해 주세요.",
  file_snapshot_changed: "파일 상태가 변경되었습니다. 다시 불러온 뒤 시도해 주세요.",
  file_changed: "디스크의 파일이 변경되었습니다. 다시 불러오거나 파일을 다시 선택해 주세요.",
  file_save_conflict: "파일이 변경되었거나 저장할 수 없습니다. 현재 내용을 보존한 채 파일 상태를 확인해 주세요.",
  file_rename_conflict: "파일 상태나 새 이름을 확인한 뒤 다시 시도해 주세요.",
  file_delete_conflict: "파일이 변경되어 삭제하지 않았습니다. 상태를 다시 확인해 주세요.",
  file_read_failed: "파일을 읽지 못했습니다. 연결 상태와 파일 크기·인코딩을 확인해 주세요.",
  file_target_unavailable: "이 파일의 실행 환경을 아직 연결하지 못했습니다.",
  unsafe_file_path: "연결된 경로 대신 실제 파일을 선택해 주세요.",
  file_limit: "열린 파일을 일부 닫은 뒤 다시 시도해 주세요.",
  file_path_limit: "이 파일의 경로가 너무 깊습니다.",
  invalid_file_choices: "최근 파일 정보를 읽지 못했습니다. 저장된 정보를 확인해 주세요.",
  files_store_changed: "편집기 저장소가 변경되었습니다. 앱을 다시 시작해 주세요.",
  files_initialize_failed: "편집기 저장소를 준비하지 못했습니다. 저장된 정보를 확인해 주세요.",
  recovery_preview_stale: "복구 대상이 변경되었습니다. 미리보기를 다시 확인해 주세요.",
  recovery_unavailable: "저장된 복구 내용을 확인하지 못했습니다.",
  request_expired: "작업 대기 시간이 초과되었습니다. 다시 시도해 주세요.",
};
export function issueMessage(issue: string): string { return issues[issue] ?? "작업을 완료하지 못했습니다. 상태를 확인하고 다시 시도해 주세요."; }
export async function nativeCall<T>(component: string, method: string, args: Record<string, unknown> = {}, route = "overview"): Promise<T> {
  const description = await describe("workspace");
  return componentCall(description, component, method, args, route);
}
/** Feature actions bind the description currently displayed by the caller. */
export async function componentCall<T>(description: Description, component: string, method: string, args: Record<string, unknown>, route: string): Promise<T> {
  const header = makeRequest(description.handshake, route, Date.now(), description.context);
  const provenance = { product: "workspace", component, requestId: header.requestId, revision: catalog.catalogRevision };
  let response: {operation: unknown; value: T & {issue?: string}};
  try {response = await invoke("plugin:workspace|execute", {request:{header, component, method, args}});}
  catch (problem) {throw new WorkspaceOperationError(problemMessage(problem, provenance));}
  if (!response || !isOperation(response.operation, provenance)) throw new Error("응답을 확인하지 못했습니다.");
  if (response.operation.outcome.state !== "succeeded") throw new WorkspaceOperationError(issueMessage(response.value?.issue ?? "operation_failed"));
  return response.value;
}
