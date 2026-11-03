import { type Messages } from "./messageDetail";

type Domain = "git" | "lsp" | "files" | "deps";
/** Messages for source editing: files, Git, language servers and dependencies. */
export const SOURCE_MESSAGES: Messages<Domain> = {
  "git.conflict_stale": {
    title: "파일 또는 Git 상태가 바뀌어 적용하지 않았습니다.",
    action: "작성한 해결안은 보존했습니다. 새 상태와 비교한 뒤 다시 적용하세요.",
  },
  "git.conflict_partial": {
    title: "충돌 해결을 끝내지 못했습니다.",
    action:
      "작성한 해결안을 보존했습니다. 현재 파일과 Git 상태를 다시 확인하세요. 복구 사본이 있으면 같은 폴더의 .devbox-conflict- 파일에 남아 있습니다.",
  },
  "git.invalid_target": { title: "Git 저장소를 찾을 수 없습니다." },
  "git.source_invalid_input": { title: "Git 요청이 올바르지 않습니다." },
  "git.source_failed": { title: "Git 작업을 완료하지 못했습니다. 저장소 상태와 인증 설정을 확인해 주세요." },
  "git.source_limit_exceeded": { title: "Git 요청의 크기 또는 작업 수가 한도를 넘었습니다." },
  "git.source_timed_out": { title: "Git 작업 시간이 초과되었습니다. 현재 상태를 확인해 주세요." },
  "git.state_changed": { title: "Git 상태가 변경되었습니다. 새로 확인한 뒤 다시 시도해 주세요." },
  "git.source_busy": { title: "이 저장소에서 다른 Git 작업이 진행 중입니다." },
  "git.unsafe_source": { title: "현재 상태에서는 이 Git 작업을 실행할 수 없습니다." },
  "lsp.invalid_input": { title: "언어 서버 요청이 올바르지 않습니다." },
  "lsp.project_not_found": { title: "언어 서버를 실행할 프로젝트를 찾을 수 없습니다." },
  "lsp.missing_server": { title: "언어 서버가 설치되어 있지 않습니다." },
  "lsp.failed": { title: "언어 서버를 시작하지 못했습니다. 다시 시작해 주세요." },
  "lsp.limit_exceeded": { title: "언어 서버 요청이 한도를 넘었습니다. 연결을 다시 열어 주세요." },
  "lsp.lease_lost": { title: "다른 창이 이 파일을 편집하고 있습니다." },
  "lsp.outside_project": { title: "이 프로젝트에서 언어 서버로 열 수 없는 경로입니다." },
  "lsp.protocol": { title: "언어 서버 응답을 읽지 못했습니다. 다시 시작해 주세요." },
  "lsp.stream_unavailable": { title: "언어 서버 연결을 열 수 없습니다." },
  "lsp.install_unavailable": { title: "언어 서버 설치 터미널을 열지 못했습니다." },
  "files.editor_unavailable": { title: "편집기를 찾지 못했습니다. 편집기 설치와 WSL 연결을 확인하세요." },
  "files.invalid_input": { title: "파일 이름이나 경로가 올바르지 않습니다." },
  "files.out_of_scope": { title: "이 프로젝트에서 열 수 없는 경로입니다." },
  "files.not_found": { title: "파일이나 폴더를 찾을 수 없습니다." },
  "files.exists": { title: "같은 이름의 파일이 이미 있습니다." },
  "files.state_changed": { title: "파일 상태가 바뀌었습니다. 새로고침한 뒤 다시 확인하세요." },
  "files.not_empty": { title: "빈 폴더만 삭제할 수 있습니다." },
  "files.limit_exceeded": { title: "폴더 항목이 너무 많습니다. 하위 폴더를 열어 주세요." },
  "deps.invalid_project": { title: "등록된 프로젝트를 찾을 수 없습니다." },
  "deps.no_lockfile": { title: "지원하는 의존성 잠금 파일이 없습니다." },
  "deps.unsupported_lock": {
    title: "지원하지 않는 잠금 파일 형식입니다.",
    action: "지원 버전과 프로젝트 설정 파일을 확인해 주세요.",
  },
  "deps.malformed_lock": { title: "잠금 파일의 형식이나 의존 관계를 읽을 수 없습니다." },
  "deps.missing_manifest": { title: "직접 의존성 확인에 필요한 프로젝트 설정 파일이 없습니다." },
  "deps.limit_exceeded": { title: "의존성 파일의 크기나 항목 수가 허용 범위를 넘었습니다." },
  "deps.unsafe_path": { title: "등록된 프로젝트 밖의 의존성 파일은 읽을 수 없습니다." },
  "deps.read_failed": { title: "의존성 파일을 읽지 못했습니다.", action: "파일을 확인하고 다시 시도하세요." },
  "deps.busy": { title: "의존성을 조회하는 중입니다.", action: "현재 조회가 끝난 뒤 다시 시도하세요." },
  "deps.job_not_found": { title: "의존성 조회 작업을 찾을 수 없습니다.", action: "다시 조회하세요." },
};
