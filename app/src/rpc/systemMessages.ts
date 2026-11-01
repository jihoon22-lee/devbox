import { type Messages, restart } from "./messageDetail";

type Domain = "runtime" | "observe" | "activity" | "debug" | "system" | "tools" | "windows";
/** Messages for runs, observation, activity, diagnostics and the environment. */
export const SYSTEM_MESSAGES: Messages<Domain> = {
  "tools.support_failed": {
    title: "지원 번들을 만들지 못했습니다.",
    action: "로그·설정 경로와 저장 공간을 확인해 주세요.",
  },
  "tools.support_too_large": { title: "지원 번들이 안전한 크기 제한을 초과했습니다." },
  "tools.invalid_input": { title: "환경 점검 항목을 확인해 주세요." },
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
  // A run's leftover processes, build servers included, keep its Windows copy in use (I1).
  "runtime.windows_copy_busy": {
    title: "Windows 사본을 쓰는 실행이나 그 실행이 남긴 프로세스(빌드 서버 등)가 아직 끝나지 않았습니다.",
    action:
      "끝날 때까지 기다린 뒤 다시 실행하세요. 빌드 서버(dotnet build의 VBCSCompiler·MSBuild, Gradle 등)는 한동안 쓰이지 않아야 스스로 끝나므로, 바로 끝내려면 dotnet build-server shutdown이나 gradle --stop을 실행하거나 작업 관리자에서 끝내세요. 모두 끝났는데도 계속되면 Windows 앱 연결을 확인해 주세요.",
  },
  "windows.config_invalid": {
    title: "Windows 설정을 읽지 못해 Windows 사본으로 실행하지 않았습니다.",
    action: `config.local.toml의 [windows] 항목이나 설정 파일 형식을 고친 뒤 ${restart}`,
  },
  "windows.observer_unavailable": {
    title: "Windows 작업 관찰을 준비하지 못했습니다.",
    action: "다시 실행하세요. 계속되면 설정 → 앱 진단을 확인하세요.",
  },
  "runtime.windows_copy_unavailable": {
    title: "Windows 실행 폴더를 안전하게 준비하지 못했습니다.",
    action: "Windows 앱 연결과 사본 루트 설정, 폴더 권한을 확인한 뒤 다시 시도해 주세요.",
  },
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
  "activity.invalid_rules": {
    title: "개인정보 규칙에 고칠 항목이 있습니다.",
    action: "표시된 규칙을 고친 뒤 다시 저장해 주세요.",
  },
  "activity.invalid_range": {
    title: "활동 기록 기간이 올바르지 않습니다.",
    action: "시작일과 종료일을 확인해 주세요.",
  },
  "activity.invalid_input": { title: "활동 기록 요청 입력값을 확인해 주세요." },
  "activity.config_unavailable": {
    title: "설정 파일에 규칙을 저장하지 못했습니다.",
    action: "config.toml이 일반 파일인지, TOML 형식이 맞는지 확인해 주세요.",
  },
  "system.unavailable": { title: "데몬 정보를 읽지 못했습니다." },
};
