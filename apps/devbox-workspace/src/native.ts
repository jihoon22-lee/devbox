import { invoke } from "@tauri-apps/api/core";
import { describe, makeRequest, type Description } from "@devbox/product-shell/api";
import { isOperation, problemMessage } from "@devbox/product-shell/operation";
import { WorkspaceOperationError } from "@devbox/workspace-features/transport";
import catalog from "../../products.json";

const issues: Record<string, string> = {
  source_review_required: "Git 설정이나 실행 근거를 다시 검토해 주세요.",
  source_requires_repository: "이 프로젝트 폴더에는 Git 저장소가 없습니다.",
  source_context_changed: "프로젝트 또는 Git 실행 근거가 변경되었습니다. 승인 상태를 다시 확인해 주세요.",
  source_preview_stale: "Git 실행 검토가 만료되었거나 이미 사용되었습니다. 다시 검토해 주세요.",
  source_trust_invalid: "저장된 Git 승인을 확인할 수 없습니다. 기존 파일을 보존했습니다.",
  source_operation_unavailable: "Git 작업을 완료하지 못했습니다. 저장소 상태와 실행 승인을 확인해 주세요.",
  git_source_limit: "Git 설정·hook의 파일 수나 크기가 확인 가능한 범위를 넘었습니다.",
  git_config_invalid: "Git 설정 파일의 형식을 확인할 수 없습니다.",
  git_environment_invalid: "Git에 전달할 HOME과 실행 환경을 확인할 수 없습니다. Git 환경을 확인한 뒤 다시 검토해 주세요.",
  git_source_unavailable: "Git 실행 파일·설정·hook을 읽지 못했습니다. 파일과 연결 상태를 확인해 주세요.",
  git_installation_unavailable: "Git for Windows 실행 파일과 설치 폴더를 확인할 수 없습니다. 설치 후 다시 검토해 주세요.",
  git_source_path_unsupported: "Git 설정 또는 hook이 확인할 수 없는 경로를 참조합니다. 실제 로컬 경로를 확인해 주세요.",
  dependency_busy: "다른 Dependency Lens 분석 또는 원격 조회가 진행 중입니다.",
  dependency_review_required: "전송 내용을 다시 검토해 주세요.",
  dependency_context_changed: "프로젝트 연결이 변경되었습니다. 다시 선택한 뒤 분석해 주세요.",
  dependency_operation_failed: "의존성 분석이나 조회를 완료하지 못했습니다. 파일과 저장된 캐시 상태를 확인하고 다시 검토해 주세요.",
  definition_owner_reference_required: "비밀 및 API 환경 참조는 해당 연결 화면에서 변경해 주세요.",
  session_task_conflict: "세션이 참조하는 작업을 먼저 조정해 주세요.",
  unsupported_overlay_version: "지원하지 않는 로컬 설정 버전입니다. 기존 파일은 보존됩니다.",
  project_definition_changed: "프로젝트 정의나 참조 파일이 변경되었습니다. 다시 확인해 주세요.",
  project_definition_unavailable: "프로젝트 정의의 참조 파일을 읽지 못했습니다. 경로와 연결 상태를 확인해 주세요.",
  project_definition_limit: "프로젝트 정의의 파일 수나 크기가 허용 범위를 넘었습니다.",
  unsafe_project_definition: "프로젝트 밖이나 연결된 경로의 정의는 사용할 수 없습니다.",
  definition_preview_stale: "프로젝트 정의 확인이 만료되었습니다. 다시 검토해 주세요.",
  unsupported_manifest_version: "지원하지 않는 프로젝트 정의 버전입니다. 기존 파일은 보존됩니다.",
  invalid_manifest: "프로젝트 정의 JSON을 읽지 못했습니다. 기존 파일은 보존됩니다.",
  invalid_manifest_definition: "프로젝트 정의의 작업·도구·참조 경로를 확인해 주세요.",
  invalid_overlay: "이 컴퓨터의 프로젝트 설정을 읽지 못했습니다. 기존 파일은 보존됩니다.",
  stale_overlay_binding: "이 컴퓨터의 설정이 이전 프로젝트 연결을 사용합니다. 다시 검토해 주세요.",
  initializing: "저장된 정보를 불러오고 있습니다.",
  busy: "앞선 작업이 끝난 뒤 다시 시도해 주세요.",
  context_busy: "파일 또는 Git 작업이 진행 중입니다. 완료나 취소를 확인한 뒤 다시 시도해 주세요.",
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
  file_owner_path: "제품이 관리하는 설정과 승인 파일은 일반 편집기로 변경할 수 없습니다. 해당 설정 화면을 사용해 주세요.",
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
  // Stay inside the native 30-second ceiling across renderer/native clock precision.
  if (component === "workspace.dependencies" || component === "workspace.source") header.deadlineMs += 24_000;
  const provenance = { product: "workspace", component, requestId: header.requestId, revision: catalog.catalogRevision };
  let response: {operation: unknown; value: T & {issue?: string}};
  try {response = await invoke("plugin:workspace|execute", {request:{header, component, method, args}});}
  catch (problem) {throw new WorkspaceOperationError(problemMessage(problem, provenance));}
  if (!response || !isOperation(response.operation, provenance)) throw new Error("응답을 확인하지 못했습니다.");
  if (response.operation.outcome.state !== "succeeded") throw new WorkspaceOperationError(issueMessage(response.value?.issue ?? "operation_failed"));
  return response.value;
}
