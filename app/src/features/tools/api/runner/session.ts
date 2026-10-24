/** 실행기 세션 값: 응답에서 캡처한 값과 그 출처(어느 실행의 어느 요청). 화면 메모리에만 둔다. */
import { VARIABLE_NAME } from "../lib/captures";

export interface Provenance {
  runId: string;
  requestId: string;
}

export interface SessionValue {
  value: string;
  from: Provenance | "environment";
}

const MAX_SESSION_VALUE_BYTES = 64 * 1024;

export class Session {
  private values = new Map<string, SessionValue>();
  private discarded = new Set<string>();

  get(name: string): SessionValue | undefined {
    return this.values.get(name);
  }

  set(name: string, value: string, from: Provenance): void {
    if (!VARIABLE_NAME.test(name) || new TextEncoder().encode(value).length > MAX_SESSION_VALUE_BYTES)
      throw new Error("세션 변수가 올바르지 않습니다");
    this.values.set(name, { value, from: { ...from } });
    this.discarded.delete(name);
  }

  /** 사용자가 지운 값. 다시 캡처할 때까지 환경 값으로 대신하지 않는다(v0.9.0 tombstone). */
  discard(names?: string[]): void {
    for (const name of names ?? [...this.values.keys()]) {
      this.values.delete(name);
      this.discarded.add(name);
    }
  }

  isDiscarded(name: string): boolean {
    return this.discarded.has(name);
  }

  entries(): [string, SessionValue][] {
    return [...this.values];
  }
}
