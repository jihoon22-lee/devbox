import type { ErrorCode } from "./client";
import type { RpcErrorCode } from "./gen/rpc";
export interface Message {
  title: string;
  action?: string;
}
const path = (d: unknown) => (d && typeof d === "object" && "path" in d && typeof d.path === "string" ? d.path : "");
const files = (detail: unknown) =>
  detail && typeof detail === "object" && "files" in detail && Array.isArray(detail.files)
    ? detail.files.filter((file): file is string => typeof file === "string").join(", ")
    : "";
const cloneMessage = (detail: unknown) => {
  if (!detail || typeof detail !== "object" || !("message" in detail) || typeof detail.message !== "string") return "";
  return ` ${detail.message
    .replace(/([a-z]+:\/\/)[^\s/]*@/gi, "$1[인증 정보 숨김]@")
    .replace(/([?&](?:token|password|access_token)=)[^&\s]+/gi, "$1[숨김]")}`;
};
const TABLE: Record<RpcErrorCode, Message | ((detail: unknown) => Message)> = {
  internal: { title: "예상하지 못한 문제가 생겼습니다.", action: "진단 ID를 복사해 두면 원인을 찾을 수 있습니다." },
  invalid_params: { title: "요청 형식이 맞지 않습니다.", action: "앱과 데몬 버전을 확인해 주세요." },
  method_not_found: { title: "이 기능을 사용할 수 없습니다.", action: "앱을 다시 시작해 주세요." },
  hello_required: { title: "연결 준비가 끝나지 않았습니다.", action: "잠시 뒤 다시 시도해 주세요." },
  cancelled: { title: "요청이 취소되었습니다." },
  daemon_restarting: { title: "다시 시작하는 중이라 실행하지 않았습니다.", action: "잠시 뒤 다시 시도해 주세요." },
  not_found: { title: "대상을 찾을 수 없습니다." },
  secrets_locked: { title: "비밀 값을 아직 열지 못했습니다.", action: "앱을 연 상태에서 다시 시도해 주세요." },
  "secrets.key_mismatch": {
    title: "이 PC의 비밀 키가 저장된 값과 다릅니다.",
    action: "복구 문자열을 입력해 기존 키를 복구해 주세요.",
  },
  "secrets.invalid_input": { title: "비밀 이름이나 입력 형식이 올바르지 않습니다." },
  "secrets.missing": { title: "이 PC에 비밀 값이 없습니다.", action: "설정에서 필요한 비밀 값을 입력해 주세요." },
  "secrets.unavailable": {
    title: "비밀 저장소를 사용할 수 없습니다.",
    action: "앱 연결과 저장소 상태를 확인해 주세요.",
  },
  "runtime.secret_missing": {
    title: "실행에 필요한 비밀 값이 없습니다.",
    action: "설정에서 필요한 비밀 값을 입력해 주세요.",
  },
  "debug.unsupported": {
    title: "시험용 스트림을 열 수 없습니다.",
    action: "시험 인스턴스의 연결 상태를 확인해 주세요.",
  },
  "runtime.def_invalid": {
    title: "실행 정의가 올바르지 않습니다.",
    action: "명령과 의존 관계, 예약 시각을 확인해 주세요.",
  },
  "runtime.project_not_found": {
    title: "실행 정의의 프로젝트를 찾을 수 없습니다.",
    action: "프로젝트를 다시 선택해 주세요.",
  },
  "runtime.project_config_invalid": {
    title: "프로젝트 실행 설정을 읽거나 저장할 수 없습니다.",
    action: ".devbox/devbox.toml의 내용과 쓰기 권한을 확인해 주세요.",
  },
  "runtime.run_not_found": { title: "실행을 찾을 수 없습니다.", action: "실행 목록을 새로고침해 주세요." },
  "runtime.not_ready": { title: "서비스가 준비되지 않았습니다.", action: "선행 서비스의 포트와 로그를 확인해 주세요." },
  "runtime.dependency_failed": {
    title: "선행 실행이 실패해 시작하지 않았습니다.",
    action: "실패한 실행의 로그를 확인해 주세요.",
  },
  "runtime.detect_parse": {
    title: "작업 감지 파일의 형식이 올바르지 않습니다.",
    action: "package.json의 scripts 항목과 JSON 문법을 확인해 주세요.",
  },
  "runtime.detect_io": {
    title: "작업 감지 파일을 읽을 수 없습니다.",
    action: "파일의 읽기 권한과 UTF-8 형식을 확인해 주세요.",
  },
  "runtime.detect_limit": {
    title: "작업 감지 파일이나 제안 목록이 너무 큽니다.",
    action: "파일은 1MiB, 제안 목록은 768KiB 이내로 줄여 주세요.",
  },
  "runtime.container_invalid_id": { title: "컨테이너 ID가 올바르지 않습니다.", action: "목록을 새로고침해 주세요." },
  "runtime.container_unavailable": {
    title: "Docker를 사용할 수 없습니다.",
    action: "Docker 설치와 실행 상태를 확인해 주세요.",
  },
  "runtime.container_failed": {
    title: "컨테이너 작업에 실패했습니다.",
    action: "Docker 상태를 확인하고 다시 시도해 주세요.",
  },
  "runtime.container_timeout": {
    title: "Docker 응답 시간이 초과되었습니다.",
    action: "목록을 새로고침하여 상태를 확인해 주세요.",
  },
  "runtime.container_output_limit": { title: "Docker 응답이 너무 큽니다.", action: "Docker 상태를 확인해 주세요." },
  "runtime.container_invalid_output": {
    title: "Docker 응답을 읽을 수 없습니다.",
    action: "Docker 상태를 확인하고 다시 시도해 주세요.",
  },
  "runtime.container_cancelled": {
    title: "컨테이너 요청이 취소되었습니다.",
    action: "목록을 새로고침하여 상태를 확인해 주세요.",
  },
  "runtime.invalid_cursor": { title: "실행 목록 위치가 올바르지 않습니다.", action: "목록을 새로고침해 주세요." },
  "terminal.command_too_long": {
    title: "터미널 명령이 너무 깁니다.",
    action: "명령을 UTF-8 기준 64KiB 이내로 줄여 주세요.",
  },
  "terminal.command_invalid": {
    title: "터미널 명령에 사용할 수 없는 문자가 있습니다.",
    action: "널 문자를 제거해 주세요.",
  },
  "terminal.invalid_cursor": { title: "터미널 목록 위치가 올바르지 않습니다.", action: "목록을 새로고침해 주세요." },
  "terminal.list_changed": { title: "조회 중 터미널 목록이 변경되었습니다.", action: "목록을 다시 조회해 주세요." },
  "terminal.title_too_long": { title: "터미널 제목이 너무 깁니다.", action: "제목을 256자 이내로 줄여 주세요." },
  "terminal.invalid_size": { title: "터미널 크기가 올바르지 않습니다.", action: "가로와 세로를 확인해 주세요." },
  "terminal.stream_unavailable": { title: "터미널 스트림을 열 수 없습니다.", action: "다시 연결해 주세요." },
  "terminal.not_found": { title: "터미널을 찾을 수 없습니다.", action: "목록을 새로 확인해 주세요." },
  "terminal.project_not_found": {
    title: "터미널의 프로젝트를 찾을 수 없습니다.",
    action: "프로젝트를 다시 선택해 주세요.",
  },
  "terminal.cwd_invalid": {
    title: "터미널 시작 폴더를 열 수 없습니다.",
    action: "WSL 안의 폴더 경로를 확인해 주세요.",
  },
  "terminal.agent_owned": {
    title: "에이전트가 소유한 터미널입니다.",
    action: "에이전트 화면에서 작업을 종료해 주세요.",
  },
  "agents.input_too_large": {
    title: "지시문이나 누적 이력이 너무 큽니다.",
    action: "지시를 줄이거나 새 작업에서 계속하세요.",
  },
  "agents.pr_title_too_long": {
    title: "PR 제목이 너무 깁니다.",
    action: "제목을 UTF-8 기준 64KiB 이내로 줄여 주세요.",
  },
  "agents.squash_message_too_long": {
    title: "병합 커밋 메시지가 너무 깁니다.",
    action: "메시지를 UTF-8 기준 64KiB 이내로 줄여 주세요.",
  },
  "agents.teardown_command_too_long": {
    title: "정리 명령이 너무 깁니다.",
    action: "명령을 UTF-8 기준 64KiB 이내로 줄여 주세요.",
  },
  "agents.teardown_command_invalid": {
    title: "정리 명령에 사용할 수 없는 문자가 있습니다.",
    action: "널 문자를 제거해 주세요.",
  },
  "agents.delivery_unknown": {
    title: "지시 전달 결과를 확인할 수 없습니다.",
    action: "터미널을 확인한 뒤 새 지시를 보내세요. 자동으로 다시 보내지 않습니다.",
  },
  "agents.resume_unavailable": {
    title: "에이전트를 이어서 실행할 수 없습니다.",
    action: "이전 도구 실행을 확인할 수 없습니다. 새 작업을 만들어 주세요.",
  },
  "agents.review_unavailable": {
    title: "고정된 검토 객체를 찾을 수 없습니다.",
    action: "저장된 작업과 커밋 기록은 유지됩니다. Git 객체 보관 상태를 확인해 주세요.",
  },
  "agents.discard_snapshot_unavailable": {
    title: "삭제할 파일의 상태를 확인하지 못했습니다.",
    action: "파일을 읽을 수 있는지와 크기를 확인한 뒤 다시 검토하세요.",
  },
  "agents.list_changed": { title: "목록을 읽는 동안 작업이 바뀌었습니다.", action: "목록을 다시 확인해 주세요." },
  "agents.project_trust_required": {
    title: "프로젝트 명령 실행 승인이 필요합니다.",
    action: "명령 내용을 다시 확인하고 승인한 뒤 실행해 주세요.",
  },
  "projects.trust_required": {
    title: "프로젝트 명령 실행 승인이 필요합니다.",
    action: "명령 내용을 다시 확인하고 승인한 뒤 실행해 주세요.",
  },
  "projects.trust_preview_stale": {
    title: "프로젝트 경로나 설정이 바뀌었습니다.",
    action: "최신 명령 내용을 확인하고 다시 승인해 주세요.",
  },
  "projects.trust_context_invalid": {
    title: "명령 실행 위치를 확인할 수 없습니다.",
    action: "프로젝트와 작업 폴더를 다시 선택해 주세요.",
  },
  "projects.in_use": {
    title: "프로젝트에 에이전트 작업, 터미널 또는 이 PC 실행 정의가 남아 있습니다.",
    action: "열린 작업과 터미널을 끝내고 남은 작업 폴더, 브랜치와 이 PC 실행 정의를 정리한 뒤 다시 시도하세요.",
  },
  "agents.title_invalid": {
    title: "에이전트 작업 제목을 확인해 주세요.",
    action: "120자 이내의 제목을 입력해 주세요.",
  },
  "agents.slug_exhausted": {
    title: "같은 이름의 에이전트 작업이 너무 많습니다.",
    action: "다른 제목을 입력해 주세요.",
  },
  "agents.not_found": { title: "에이전트 작업을 찾을 수 없습니다.", action: "목록을 다시 확인해 주세요." },
  "agents.project_not_found": {
    title: "에이전트 작업의 프로젝트를 찾을 수 없습니다.",
    action: "프로젝트를 다시 선택해 주세요.",
  },
  "agents.profile_not_found": {
    title: "에이전트 도구를 찾을 수 없습니다.",
    action: "사용할 도구를 다시 선택해 주세요.",
  },
  "agents.bad_state": {
    title: "현재 상태에서는 실행할 수 없습니다.",
    action: "에이전트 작업 상태를 다시 확인해 주세요.",
  },
  "agents.worktree_dirty": {
    title: "작업 폴더에 커밋하지 않은 변경이 있습니다.",
    action: "변경을 확인하고 커밋한 뒤 다시 시도해 주세요.",
  },
  "agents.base_dirty": (detail) => ({
    title: "기준 체크아웃에 커밋하지 않은 변경이 있어 병합하지 않았습니다.",
    action: `기준 폴더의 변경을 커밋하거나 치운 뒤 다시 시도해 주세요.${files(detail) ? ` 변경 파일: ${files(detail)}` : ""}`,
  }),
  "agents.base_not_checked_out": {
    title: "기준 브랜치가 열려 있지 않습니다.",
    action: "프로젝트 폴더에서 기준 브랜치를 선택해 주세요.",
  },
  "agents.conflict": (detail) => ({
    title: "변경 내용이 충돌해 작업을 멈췄습니다.",
    action: `충돌 파일을 확인하고 기준 반영으로 해결해 주세요.${files(detail) ? ` 충돌 파일: ${files(detail)}` : ""}`,
  }),
  "agents.unmerged_commits": {
    title: "아직 병합하지 않은 커밋이 있습니다.",
    action: "폐기할 변경 내용을 확인해 주세요.",
  },
  "agents.gh_missing": { title: "GitHub CLI를 찾을 수 없습니다.", action: "WSL에 gh를 설치하고 로그인해 주세요." },
  "agents.not_repo": { title: "Git 저장소가 아닙니다.", action: "프로젝트 폴더를 확인해 주세요." },
  "agents.state_changed": {
    title: "확인한 뒤 작업이 바뀌었습니다.",
    action: "바뀐 내용을 확인하고 다시 실행하세요.",
  },
  "agents.operation_in_progress": {
    title: "다른 Git 작업이 진행 중입니다.",
    action: "진행 중인 작업을 마친 뒤 다시 시도해 주세요.",
  },
  "agents.unsafe_worktree": {
    title: "작업 폴더의 소유 상태를 확인할 수 없습니다.",
    action: "등록된 작업 폴더와 경로를 확인해 주세요.",
  },
  "agents.branch_exists": {
    title: "같은 이름의 브랜치가 이미 있습니다.",
    action: "다른 제목으로 에이전트 작업을 만들어 주세요.",
  },
  "agents.worktree_add_partial": {
    title: "작업 폴더 준비가 일부만 완료되었습니다.",
    action: "남아 있는 폴더와 브랜치를 확인해 주세요.",
  },
  "agents.invalid_input": {
    title: "에이전트 작업의 입력값이 올바르지 않습니다.",
    action: "입력한 값을 확인해 주세요.",
  },
  "agents.session_missing": { title: "에이전트 실행이 끊겼습니다.", action: "에이전트 작업을 재개해 주세요." },
  "agents.session_metadata": {
    title: "에이전트의 대화 기록을 확인하지 못했습니다.",
    action: "잠시 뒤 상태를 다시 확인해 주세요.",
  },
  "system.env_refresh_failed": {
    title: "셸 환경을 다시 읽지 못했습니다.",
    action: "로그인 셸 설정과 환경 파일 위치를 확인한 뒤 다시 시도해 주세요.",
  },
  "observe.invalid_input": { title: "관찰 요청 입력값을 확인해 주세요." },
  "observe.state_changed": { title: "프로세스가 바뀌었습니다.", action: "목록을 새로 고친 뒤 다시 확인해 주세요." },
  "observe.permission_denied": { title: "이 프로세스를 종료할 권한이 없습니다." },
  "observe.project_not_found": { title: "관찰할 프로젝트를 찾을 수 없습니다." },
  "observe.invalid_regex": { title: "로그 검색 정규식이 올바르지 않습니다." },
  "observe.file_outside_roots": { title: "등록한 프로젝트·노트 폴더·/var/log 안의 파일만 읽을 수 있습니다." },
  "observe.source_unavailable": {
    title: "관찰 정보를 읽을 수 없습니다.",
    action: "프로세스·파일 경로와 컨테이너 상태를 확인해 주세요.",
  },
  "observe.run_not_found": { title: "실행 기록을 찾을 수 없습니다." },
  "observe.invalid_cursor": { title: "로그 파일이 바뀌었습니다.", action: "처음부터 다시 검색해 주세요." },
  "observe.limit_exceeded": {
    title: "조회가 허용 크기를 넘었습니다.",
    action: "조회 범위를 줄인 뒤 다시 시도해 주세요.",
  },
  "observe.stream_unsupported": { title: "이 연결에서는 실시간 로그를 볼 수 없습니다." },
  "documents.conflict": {
    title: "다른 곳에서 이 문서를 고쳤습니다.",
    action: "디스크 내용과 비교한 뒤 어느 쪽을 남길지 골라 주세요. 편집 내용은 보존했습니다.",
  },
  "documents.lease_lost": (d) =>
    d && typeof d === "object" && "reason" in d && d.reason === "taken"
      ? { title: "다른 창이 이 문서를 가져갔습니다.", action: "[여기로 가져오기]로 편집을 이 창으로 옮길 수 있습니다." }
      : { title: "이 창의 편집 권한이 만료되었습니다.", action: "문서를 다시 열어 주세요. 편집 내용은 보존했습니다." },
  "documents.out_of_scope": {
    title: "등록한 프로젝트·에이전트 작업 폴더·노트 폴더 안의 문서만 열거나 저장할 수 있습니다.",
  },
  "documents.not_found": { title: "문서를 찾을 수 없습니다." },
  "documents.exists": { title: "같은 이름의 파일이 이미 있습니다.", action: "다른 이름을 골라 주세요." },
  "documents.too_large": { title: "문서가 허용 크기를 넘었습니다.", action: "내용을 줄인 뒤 다시 시도해 주세요." },
  "documents.stale_journal": { title: "더 새로운 임시 저장본이 이미 있습니다." },
  "documents.unsupported": {
    title: "이 폴더의 파일 시스템은 안전한 저장을 지원하지 않습니다.",
    action: "WSL 안의 폴더(/home/...)에 문서를 두어 주세요.",
  },
  "documents.invalid_input": { title: "문서 요청 입력값을 확인해 주세요." },
  "notes.not_found": { title: "노트를 찾을 수 없습니다." },
  "notes.exists": { title: "같은 이름의 노트나 파일이 이미 있습니다.", action: "다른 이름을 골라 주세요." },
  "notes.out_of_vault": { title: "노트 폴더 안의 파일만 다룰 수 있습니다." },
  "notes.invalid_name": { title: "노트 이름으로 쓸 수 있는 글자가 없습니다.", action: "다른 제목을 입력해 주세요." },
  "notes.invalid_template": {
    title: "템플릿을 적용하지 못했습니다.",
    action: "템플릿 파일과 변수 이름을 확인해 주세요.",
  },
  "notes.invalid_image": {
    title: "이미지를 첨부하지 못했습니다.",
    action: "2MB 이하의 PNG·JPEG·GIF·WebP만 첨부할 수 있습니다.",
  },
  "notes.invalid_input": { title: "노트 요청 입력값을 확인해 주세요." },
  "notes.busy": { title: "노트 링크를 고치는 중입니다.", action: "잠시 뒤 다시 시도해 주세요." },
  "notes.target_unsaved": {
    title: "새 이름의 노트에 저장하지 않은 편집이 있습니다.",
    action: "그 노트를 열어 저장하거나 버린 뒤 다시 시도해 주세요.",
  },
  "notes.stale_cursor": { title: "노트 목록이 바뀌었습니다.", action: "목록을 처음부터 다시 불러와 주세요." },
  "notes.too_large": { title: "노트가 허용 크기를 넘었습니다.", action: "내용을 줄인 뒤 다시 시도해 주세요." },
  "notes.open_unsaved": {
    title: "노트에 저장하지 않은 편집이 있어 쓰지 않았습니다.",
    action: "그 노트를 저장하거나 버린 뒤 다시 시도해 주세요.",
  },
  "notes.summary_broken": {
    title: "일일 기록의 요약 표시가 깨져 있습니다.",
    action: "<!-- devbox:summary:… --> 줄과 <!-- /devbox:summary --> 줄이 짝을 이루는지 확인해 주세요.",
  },
  "notes.conflict": { title: "노트가 계속 바뀌고 있어 쓰지 못했습니다.", action: "잠시 뒤 다시 시도해 주세요." },
  "system.unavailable": { title: "데몬 정보를 읽지 못했습니다." },
  "projects.path_invalid": (d) => ({
    title: `경로를 열 수 없습니다: ${path(d)}`,
    action: "WSL 안의 절대 경로(/home/...)를 입력해 주세요.",
  }),
  "projects.not_directory": (d) => ({ title: `폴더가 아닙니다: ${path(d)}` }),
  "projects.already_exists": (d) => ({ title: `이미 등록한 프로젝트입니다: ${path(d)}` }),
  "projects.not_found": { title: "프로젝트를 찾을 수 없습니다.", action: "목록을 다시 확인해 주세요." },
  "projects.configuration_invalid": {
    title: "프로젝트 설정을 읽을 수 없습니다.",
    action: ".devbox/devbox.toml의 내용을 확인해 주세요.",
  },
  "projects.launch_profile_not_found": {
    title: "시작 구성의 에이전트 도구를 찾을 수 없습니다.",
    action: "프로젝트 시작 구성과 도구 프로필을 확인해 주세요.",
  },
  "projects.launch_failed": {
    title: "시작 구성을 일부 실행하지 못했습니다.",
    action: "이미 시작한 터미널·에이전트·서비스를 확인한 뒤 다시 시도해 주세요.",
  },
  "projects.clone_invalid_url": { title: "저장소 주소가 올바르지 않습니다." },
  "projects.clone_target_exists": (d) => ({ title: `복제 대상 폴더가 이미 있습니다: ${path(d)}` }),
  "projects.clone_failed": (d) => ({
    title: "저장소를 복제하지 못했습니다.",
    action: `저장소 주소와 접근 권한, 대상 폴더를 확인해 주세요. 남아 있는 파일은 보존했습니다.${cloneMessage(d)}`,
  }),
  "projects.unsafe_prompt_path": { title: "지시문 템플릿 경로를 안전하게 읽을 수 없습니다." },
  "projects.prompt_limit_exceeded": {
    title: "지시문 템플릿이 허용 크기를 넘었습니다.",
    action: "파일은 50개 이내, 각 파일은 64 KiB 이내로 줄여 주세요.",
  },
  "projects.prompt_invalid_encoding": { title: "지시문 템플릿은 UTF-8 형식이어야 합니다." },
};
export function messageFor(code: ErrorCode, detail?: unknown): Message {
  if (code === "connection_lost")
    return {
      title: "연결이 끊겨 실행 결과를 확인하지 못했습니다.",
      action: "다시 연결되면 결과를 확인한 뒤 재시도해 주세요.",
    };
  const entry = TABLE[code];
  return typeof entry === "function" ? entry(detail) : entry;
}
