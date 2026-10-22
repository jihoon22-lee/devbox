import { looksLikeSecret } from "./secretPatterns";

// 데몬 `template::is_reference`와 같은 규칙: 이름 앞뒤는 공백·탭만 허용(개행은 참조가 아님).
// v1 비밀 참조 `{{secret:NAME}}`(NAME은 devbox_secrets::valid_name 규칙)도 값 전체가 참조이면 직접 입력 값이 아니다.
const EXACT_VARIABLE_REFERENCE =
  /^(?:\{\{[ \t]*([a-zA-Z0-9_.-]+)[ \t]*\}\}|\$\{[ \t]*([a-zA-Z0-9_.-]+)[ \t]*\}|\{\{secret:[A-Za-z_][A-Za-z0-9_]{0,127}\}\})$/;

/** 값 전체가 참조 하나인지. `{{ghp_…}}`처럼 이름이 토큰 모양이면 참조처럼 감싼 직접 입력 비밀로 본다. */
export function isExactVariableReference(value: string): boolean {
  const match = EXACT_VARIABLE_REFERENCE.exec(value);
  if (!match) return false;
  const name = match[1] ?? match[2];
  return name === undefined || !looksLikeSecret(name);
}
