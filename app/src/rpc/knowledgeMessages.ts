import { field, type Messages, restart } from "./messageDetail";

type Domain = "documents" | "search" | "notes";
/** Messages for documents, search and notes. */
export const KNOWLEDGE_MESSAGES: Messages<Domain> = {
  "documents.not_empty": { title: "빈 폴더만 삭제할 수 있습니다." },
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
  "notes.response_unavailable": (d) =>
    field(d, "reason") === "pending"
      ? {
          title: "응답 기록을 아직 저장하고 있습니다.",
          action: "잠시 뒤 같은 작업을 다시 확인하세요. 요청을 다시 보내지는 않습니다.",
        }
      : field(d, "reason") === "origin"
        ? {
            title: "응답을 보낸 프로젝트나 컬렉션 출처가 바뀌었습니다.",
            action: "현재 출처를 확인하세요. 바뀐 기록은 노트에 붙이지 않습니다.",
          }
        : {
            title: "이 응답 기록을 노트에 붙일 수 없습니다.",
            action: "이전 세션·누락·변경·미완료 기록인지 확인하세요. 자동으로 재송신하지 않습니다.",
          },
  "notes.response_target_changed": {
    title: "선택한 노트나 보관함이 바뀌었습니다.",
    action: "현재 보관함에서 대상을 다시 선택해 주세요.",
  },
  "notes.append_uncertain": {
    title: "노트에 붙였는지 아직 확인할 수 없습니다.",
    action: "노트 창의 저장 상태를 확인한 뒤 같은 작업을 다시 확인하세요. 같은 작업을 다시 붙이지 않습니다.",
  },
  "notes.append_operation_mismatch": {
    title: "노트 추가 작업의 대상이나 응답이 일치하지 않습니다.",
    action: "원래 응답에서 대상을 다시 선택해 주세요.",
  },
  "notes.conflict": { title: "노트가 계속 바뀌고 있어 쓰지 못했습니다.", action: "잠시 뒤 다시 시도해 주세요." },
  "notes.disabled": {
    title: "노트 설정을 읽지 못해 노트를 껐습니다.",
    action: `config.toml·config.local.toml의 [notes] 항목이나 설정 파일 형식을 고친 뒤 ${restart}`,
  },
  "search.invalid_regex": { title: "정규식이 올바르지 않습니다.", action: "괄호와 특수 문자를 확인해 주세요." },
  "search.invalid_input": { title: "검색 입력값을 확인해 주세요.", action: "검색어 길이와 결과 개수를 줄여 보세요." },
  "search.project_not_found": {
    title: "검색할 프로젝트를 찾을 수 없습니다.",
    action: "프로젝트 목록을 다시 확인해 주세요.",
  },
  "search.not_found": { title: "저장된 검색을 찾을 수 없습니다." },
  "search.stream_unsupported": { title: "이 연결에서는 통합 검색을 쓸 수 없습니다.", action: "앱에서 검색해 주세요." },
  "search.disabled": {
    title: "검색 설정을 읽지 못해 검색을 껐습니다.",
    action: `config.toml·config.local.toml의 [search] 항목이나 설정 파일 형식을 고친 뒤 ${restart}`,
  },
};
