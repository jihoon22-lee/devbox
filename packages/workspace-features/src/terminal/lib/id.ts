let counter = 0;

/** 탭 등 프론트 전용 id 생성. 세션 id(Rust가 발급)와는 별개다. */
export function makeId(prefix: string): string {
  counter += 1;
  return `${prefix}${Date.now().toString(36)}${counter}`;
}
