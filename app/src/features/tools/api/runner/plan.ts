/** 실행 전 계획: 항목별로 쓰는 변수와 변수별 생산 요청(이번 실행 안, 순서대로). */
import type { RequestSnapshot } from "../../../../rpc/gen/rpc";
import { activeTexts } from "../model/draft";

export interface VariableUse {
  name: string;
  /** `{{name}}`이면 참(없으면 데몬이 거절), `${name}`만이면 거짓(없으면 글자 그대로). */
  required: boolean;
}

// 데몬 `template::variable_name`: 앞뒤 공백·탭만 자르고 영숫자·`_`·`.`·`-`.
const REFERENCE = /\{\{([ \t]*[A-Za-z0-9_.-]+[ \t]*)\}\}|\$\{([ \t]*[A-Za-z0-9_.-]+[ \t]*)\}/g;

/** 활성 필드의 변수 참조(비밀 참조 제외), 처음 나온 순서. */
export function variableRefs(snapshot: RequestSnapshot): VariableUse[] {
  const uses = new Map<string, VariableUse>();
  for (const text of activeTexts(snapshot)) {
    for (const match of text.matchAll(REFERENCE)) {
      const name = (match[1] ?? match[2]!).replace(/^[ \t]+|[ \t]+$/g, "");
      const required = match[1] !== undefined;
      const seen = uses.get(name);
      if (seen) seen.required ||= required;
      else uses.set(name, { name, required });
    }
  }
  return [...uses.values()];
}

export function variableUses(snapshot: RequestSnapshot): string[] {
  return variableRefs(snapshot).map((use) => use.name);
}

export interface RunPlan {
  uses: VariableUse[][];
  /** 변수 → 그것을 캡처하는 항목 번호(오름차순). */
  producers: Map<string, number[]>;
}

export function planRun(entries: { produces: string[]; snapshot: RequestSnapshot }[]): RunPlan {
  const producers = new Map<string, number[]>();
  entries.forEach((entry, index) => {
    for (const name of new Set(entry.produces)) producers.set(name, [...(producers.get(name) ?? []), index]);
  });
  return { uses: entries.map((entry) => variableRefs(entry.snapshot)), producers };
}

/** `index`보다 앞선 마지막 생산 요청. */
export function latestProducer(plan: RunPlan, variable: string, index: number): number | undefined {
  return plan.producers
    .get(variable)
    ?.filter((at) => at < index)
    .at(-1);
}
