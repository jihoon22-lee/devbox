import type { ErrorCode } from "./client";
import type { RpcErrorCode } from "./gen/rpc";
export interface Message {
  title: string;
  action?: string;
}
const path = (d: unknown) => (d && typeof d === "object" && "path" in d && typeof d.path === "string" ? d.path : "");
const TABLE: Record<RpcErrorCode, Message | ((detail: unknown) => Message)> = {
  internal: { title: "예상하지 못한 문제가 생겼습니다.", action: "진단 ID를 복사해 두면 원인을 찾을 수 있습니다." },
  invalid_params: { title: "요청 형식이 맞지 않습니다.", action: "앱과 데몬 버전을 확인해 주세요." },
  method_not_found: { title: "이 기능을 사용할 수 없습니다.", action: "앱을 다시 시작해 주세요." },
  hello_required: { title: "연결 준비가 끝나지 않았습니다.", action: "잠시 뒤 다시 시도해 주세요." },
  cancelled: { title: "요청이 취소되었습니다." },
  daemon_restarting: { title: "다시 시작하는 중이라 실행하지 않았습니다.", action: "잠시 뒤 다시 시도해 주세요." },
  not_found: { title: "대상을 찾을 수 없습니다." },
  secrets_locked: { title: "비밀 값을 아직 열지 못했습니다.", action: "앱을 연 상태에서 다시 시도해 주세요." },
  "debug.unsupported": {
    title: "시험용 스트림을 열 수 없습니다.",
    action: "시험 인스턴스의 연결 상태를 확인해 주세요.",
  },
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
  "agents.base_dirty": {
    title: "기준 브랜치에 커밋하지 않은 변경이 있습니다.",
    action: "기준 폴더의 변경을 확인해 주세요.",
  },
  "agents.base_not_checked_out": {
    title: "기준 브랜치가 열려 있지 않습니다.",
    action: "프로젝트 폴더에서 기준 브랜치를 선택해 주세요.",
  },
  "agents.conflict": { title: "변경 내용이 충돌합니다.", action: "충돌 파일을 확인해 주세요." },
  "agents.unmerged_commits": {
    title: "아직 병합하지 않은 커밋이 있습니다.",
    action: "폐기할 변경 내용을 확인해 주세요.",
  },
  "agents.gh_missing": { title: "GitHub CLI를 찾을 수 없습니다.", action: "WSL에 gh를 설치하고 로그인해 주세요." },
  "agents.not_repo": { title: "Git 저장소가 아닙니다.", action: "프로젝트 폴더를 확인해 주세요." },
  "agents.state_changed": {
    title: "확인한 뒤 변경 내용이 달라졌습니다.",
    action: "최신 변경 내용을 다시 확인해 주세요.",
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
  "system.unavailable": { title: "데몬 정보를 읽지 못했습니다." },
  "projects.path_invalid": (d) => ({
    title: `경로를 열 수 없습니다: ${path(d)}`,
    action: "WSL 안의 절대 경로(/home/...)를 입력해 주세요.",
  }),
  "projects.not_directory": (d) => ({ title: `폴더가 아닙니다: ${path(d)}` }),
  "projects.already_exists": (d) => ({ title: `이미 등록한 프로젝트입니다: ${path(d)}` }),
  "projects.not_found": { title: "프로젝트를 찾을 수 없습니다.", action: "목록을 다시 확인해 주세요." },
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
