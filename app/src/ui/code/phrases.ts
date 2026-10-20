import { EditorState } from "@codemirror/state";

export const codePhrases = EditorState.phrases.of({
  Find: "찾기",
  Replace: "바꾸기",
  next: "다음",
  previous: "이전",
  all: "모두 선택",
  "match case": "대소문자 구분",
  regexp: "정규식",
  "by word": "단어 단위",
  replace: "바꾸기",
  "replace all": "모두 바꾸기",
  close: "닫기",
  "Go to line": "줄로 이동",
  go: "이동",
  "Selection deleted": "선택 영역을 삭제했습니다",
});
