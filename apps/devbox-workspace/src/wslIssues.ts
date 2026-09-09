const issues:Record<string,string>={
  wsl_distro_stopped: "WSL 배포판이 중지되어 있습니다. 목록을 새로 고친 뒤 시작 여부를 선택해 주세요.",
  wsl_distro_missing: "WSL 배포판 등록을 찾을 수 없습니다. 목록을 새로 고쳐 주세요.",
  wsl_registry_changed: "WSL 배포판이나 디스크가 바뀌었습니다. 폴더 연결을 다시 검토해 주세요.",
  wsl_helper_unavailable: "WSL 연결 파일을 확인하지 못했습니다. Workspace 설치 상태를 확인해 주세요.",
  wsl_helper_changed: "WSL 연결 파일이 변경되었습니다. Workspace를 다시 설치한 뒤 확인해 주세요.",
  wsl_identity_unavailable: "이 WSL 파일시스템의 폴더 식별 정보를 확인할 수 없습니다.",
  wsl_root_changed: "WSL 폴더나 저장소가 바뀌었습니다. 폴더 연결을 다시 검토해 주세요.",
  wsl_context_invalid: "WSL 프로젝트 문맥을 확인하지 못했습니다. 프로젝트를 다시 선택해 주세요.",
  wsl_context_required: "WSL 파일을 열기 전에 프로젝트 연결을 확인해 주세요.",
  wsl_native_filesystem_required: "배포판의 기본 Linux 파일시스템 폴더를 선택해 주세요. 다른 마운트의 파일은 아직 지원하지 않습니다.",
  wsl_filesystem_unavailable: "WSL 파일시스템 정보를 읽지 못했습니다. 배포판 상태를 확인해 주세요.",
  wsl_root_expired: "WSL 폴더 검토가 만료되었습니다. 폴더를 다시 확인해 주세요.",
  wsl_connection_closed: "WSL 연결이 종료되었습니다. 배포판 상태를 확인하고 다시 연결해 주세요.",
  wsl_timeout: "WSL 연결 시간이 초과되었습니다. 배포판 상태를 확인해 주세요.",
  wsl_operation_failed: "WSL 폴더를 확인하지 못했습니다. 경로와 접근 권한을 확인해 주세요.",
  wsl_admission_required: "WSL 프로젝트 연결은 아직 사용할 수 없습니다.",
};
export function wslIssueMessage(issue:string):string|undefined{return issues[issue];}
