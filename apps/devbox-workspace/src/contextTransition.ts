export class ContextTransitionBlocked extends Error {}
export type TransitionGuard = <T>(operation: () => Promise<T>) => Promise<T>;
/** Recheck current state after asynchronous preparation and hold one mutation. */
export function createContextTransition(reasons: () => string[], flush: () => Promise<void>): TransitionGuard {
  let pending = false;
  return async <T>(operation: () => Promise<T>) => {
    if (pending) throw new ContextTransitionBlocked("프로젝트 전환을 확인 중입니다.");
    const check = () => {
      const blocked = reasons();
      if (blocked.length)
        throw new ContextTransitionBlocked(`${blocked.join(", ")} 내용을 정리한 뒤 다시 시도해 주세요.`);
    };
    check();
    pending = true;
    try {
      await flush();
      check();
      return await operation();
    } finally {
      pending = false;
    }
  };
}
