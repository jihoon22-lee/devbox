import { check, configFile, field, type Messages, names, row } from "./messageDetail";

type Domain = "api" | "webhooks" | "secrets";
/** Messages for API requests, webhooks and secrets. */
export const API_MESSAGES: Messages<Domain> = {
  "api.storage_sensitive_body": {
    title: "본문에 비밀 값이 있거나 저장 형식을 확인할 수 없습니다.",
    action: "JSON 형식을 확인하고 비밀 값은 참조({{secret:NAME}})나 [REDACTED]로 바꿔 주세요.",
  },
  "webhooks.invalid_input": { title: "웹훅 입력을 확인해 주세요." },
  "webhooks.not_found": { title: "수신 기록을 찾을 수 없습니다." },
  "webhooks.bind_failed": { title: "포트를 열 수 없습니다. 다른 포트를 선택해 주세요." },
  "webhooks.limit_reached": { title: "열 수 있는 리스너 수를 넘었습니다." },
  "webhooks.storage": { title: "웹훅 기록을 읽거나 저장하지 못했습니다." },
  "secrets.key_mismatch": {
    title: "이 PC의 비밀 키가 저장된 값과 다릅니다.",
    action: "복구 문자열을 입력해 기존 키를 복구해 주세요.",
  },
  "secrets.invalid_input": { title: "비밀 이름이나 입력 형식이 올바르지 않습니다." },
  "secrets.missing": { title: "이 PC에 비밀 값이 없습니다.", action: "설정에서 필요한 비밀 값을 입력해 주세요." },
  "secrets.config_invalid": (d) => ({
    title: "설정 파일을 읽지 못해 비밀 값을 저장하지 않았습니다.",
    action: `${configFile(d)}이 일반 파일인지, TOML 형식이 맞는지 확인해 고친 뒤 다시 시도해 주세요.`,
  }),
  "secrets.unavailable": {
    title: "비밀 저장소를 사용할 수 없습니다.",
    action: "앱 연결과 저장소 상태를 확인해 주세요.",
  },
  "api.response_export_failed": {
    title: "완료된 응답의 출처를 확인할 수 없습니다.",
    action: "기록 저장 중이면 잠시 뒤 다시 확인하세요. 이전 세션이나 변경된 기록은 내보내지 않습니다.",
  },
  "api.invalid_header": (d) => ({
    title: `헤더 ${row(d)}${field(d, "field") === "value" ? "값" : "이름"}이 올바르지 않습니다.`,
    action: check,
  }),
  "api.invalid_cookie": (d) => ({ title: `쿠키 ${row(d)}이름이나 값이 올바르지 않습니다.`, action: check }),
  "api.invalid_url": {
    title: "요청 URL이 올바르지 않습니다.",
    action: "http:// 또는 https://로 시작하고 사용자 정보가 없는지 확인해 주세요.",
  },
  "api.invalid_method": { title: "요청 메서드가 올바르지 않습니다.", action: check },
  "api.invalid_body": { title: "요청 본문이 올바르지 않습니다.", action: check },
  "api.invalid_input": { title: "요청 설정이 올바르지 않습니다.", action: check },
  "api.variable_missing": (d) => ({
    title: "정의되지 않은 변수가 있어 보내지 않았습니다.",
    action: `환경에서 변수를 정의해 주세요: ${names(d)}`,
  }),
  "api.secret_missing": {
    title: "요청에 필요한 비밀 값이 없어 보내지 않았습니다.",
    action: "설정에서 필요한 비밀 값을 입력해 주세요.",
  },
  "api.file_missing": {
    title: "보낼 파일을 찾을 수 없습니다.",
    action: "WSL 안의 절대 경로와 파일이 있는지 확인해 주세요.",
  },
  "api.too_large": { title: "요청이 허용 크기를 넘었습니다.", action: "행 수나 본문 크기를 줄여 주세요." },
  "api.redaction_failed": (d) => ({
    title: "응답의 비밀 값 보호를 완료하지 못했습니다.",
    action:
      field(d, "reason") === "capacity"
        ? "자격 증명 수나 크기를 줄인 뒤 다시 시도해 주세요."
        : field(d, "reason") === "input"
          ? "서버 응답과 인증 설정을 확인해 주세요."
          : "다시 시도해 주세요. 문제가 계속되면 앱을 다시 시작해 주세요.",
  }),
  "api.tls_invalid": {
    title: "TLS 인증서 설정을 사용할 수 없습니다.",
    action: "CA·클라이언트 인증서·키 PEM 파일을 확인해 주세요.",
  },
  "api.oauth_failed": { title: "OAuth 토큰을 받지 못했습니다.", action: "토큰 URL과 클라이언트 설정을 확인해 주세요." },
  "api.network": {
    title: "요청 전송에 실패했습니다.",
    action: "주소·네트워크·인증서와 요청 시간 제한을 확인해 주세요.",
  },
  "api.stream_unsupported": { title: "이 연결에서는 요청을 보낼 수 없습니다." },
  "api.stream_rejected": (d) => {
    const reason = field(d, "reason");
    const status = field(d, "status");
    if (reason === "unauthorized") {
      return {
        title:
          String(status) === "403"
            ? "서버가 이 요청을 허락하지 않았습니다(HTTP 403)."
            : `서버가 인증을 받지 않았습니다(HTTP ${status ?? 401}).`,
        action: "인증 설정(토큰·헤더)이나 OAuth 승인을 확인해 주세요.",
      };
    }
    return {
      title: `서버가 연결을 받지 않았습니다${reason === "status" ? `(HTTP ${status})` : reason === "content_type" ? "(이벤트 스트림 아님)" : "(하위 프로토콜 불일치)"}.`,
      action: "주소·헤더·하위 프로토콜을 확인해 주세요.",
    };
  },
  "api.too_many_streams": {
    title: "동시에 열 수 있는 실시간 연결 수를 넘었습니다.",
    action: "다른 연결을 닫은 뒤 다시 시도해 주세요.",
  },
  "api.history_not_found": { title: "요청 기록을 찾을 수 없습니다.", action: "기록 목록을 새로고침해 주세요." },
  "api.collection_path_invalid": { title: "컬렉션 파일 경로가 올바르지 않습니다.", action: "이름을 확인해 주세요." },
  "api.collection_conflict": { title: "파일이 바뀌었거나 이미 있습니다.", action: "다시 읽은 뒤 저장해 주세요." },
  "api.collection_too_large": { title: "컬렉션 파일이 1MiB를 넘었습니다.", action: "파일을 나눠 주세요." },
  "api.collection_not_found": { title: "컬렉션 파일을 찾을 수 없습니다.", action: "목록을 새로고침해 주세요." },
  "api.project_not_found": { title: "등록된 프로젝트를 찾을 수 없습니다.", action: "프로젝트 목록을 확인해 주세요." },
  "api.grpc": {
    title: "gRPC 서비스를 불러오거나 호출하지 못했습니다.",
    action: "대상·메서드·요청 JSON·proto 파일을 확인해 주세요.",
  },
  "api.review_required": {
    title: "이 PC에서 검토하지 않은 프로젝트 파일입니다.",
    action: "내용과 명령을 확인한 뒤 [검토하고 사용]을 눌러 주세요.",
  },
  "api.mcp": { title: "MCP 서버와 연결하지 못했습니다.", action: "명령·주소와 서버 응답을 확인해 주세요." },
};
