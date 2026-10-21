// v1 비밀 참조 `{{secret:NAME}}`(NAME은 devbox_secrets::valid_name 규칙)도 값 전체가 참조이면 직접 입력 값이 아니다.
const EXACT_VARIABLE_REFERENCE =
  /^(?:\{\{\s*[a-zA-Z0-9_.-]+\s*\}\}|\$\{\s*[a-zA-Z0-9_.-]+\s*\}|\{\{secret:[A-Za-z_][A-Za-z0-9_]{0,127}\}\})$/;

export function isExactVariableReference(value: string): boolean {
  return EXACT_VARIABLE_REFERENCE.test(value);
}
