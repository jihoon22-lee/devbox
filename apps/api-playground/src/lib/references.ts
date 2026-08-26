const EXACT_VARIABLE_REFERENCE = /^(?:\{\{\s*[a-zA-Z0-9_.-]+\s*\}\}|\$\{\s*[a-zA-Z0-9_.-]+\s*\})$/;

export function isExactVariableReference(value: string): boolean {
  return EXACT_VARIABLE_REFERENCE.test(value);
}
