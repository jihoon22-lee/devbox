/**
 * 컬렉션 실행기(07b S5-3, CV-F04·AR-F03). 캡처 값은 출처를 붙여 `origin: "capture"`로 보내
 * 데몬이 `{{secret:…}}` 참조로 해석하지 않게 한다.
 */
import type { EnvVar, Pair, RequestSnapshot } from "../../../../rpc/gen/rpc";
import { type AssertionResult, evaluateAssertions } from "../lib/assertions";
import { applyCaptures } from "../lib/captures";
import { isSensitiveName, looksLikeSecret } from "../lib/secretPatterns";
import { stillMasked } from "../lib/storage";
import type { ApiResponse } from "../lib/template";
import { type RequestDraft, toSnapshot } from "../model/draft";
import { toStored } from "../model/stored";
import { latestProducer, planRun, type RunPlan } from "./plan";
import type { Provenance, Session, SessionValue } from "./session";

export interface RunEntry {
  id: string;
  name: string;
  draft: RequestDraft;
  reviewNeeded: boolean;
  produces: string[];
  /** 파일에 기록된 가린 필드 위치(`request.` 접두사 경로). */
  masked?: string[];
}

export type StepStatus = "passed" | "failed" | "error" | "skipped";

export interface VariableSource {
  name: string;
  from: Provenance | "environment";
}

export interface RunStep {
  entryId: string;
  name: string;
  status: StepStatus;
  httpStatus: number | null;
  durationMs: number | null;
  assertions: AssertionResult[];
  captured: string[];
  sources: VariableSource[];
  message: string;
}

export interface RunSummary {
  runId: string;
  steps: RunStep[];
  passed: number;
  failed: number;
  errors: number;
  skipped: number;
  cancelled: boolean;
}

export interface RunOptions {
  stopOnFailure: boolean;
  delayMs: number;
}

export interface RunDeps {
  runId: string;
  send(snapshot: RequestSnapshot, variables: EnvVar[], signal: AbortSignal): Promise<ApiResponse>;
  sleep(ms: number, signal: AbortSignal): Promise<void>;
}

export const MAX_RUN_ENTRIES = 200;
export const MAX_RUN_DELAY_MS = 5000;

const LIMIT_ERROR = "러너 한도를 넘었습니다. 요청 200개, 대기 0–5000ms까지 사용할 수 있습니다.";

function envSensitive(name: string, value: string): boolean {
  return isSensitiveName(name) || looksLikeSecret(value) || value.includes("{{secret:");
}

function sameOrigin(a: SessionValue["from"] | undefined, b: Provenance): boolean {
  return typeof a === "object" && a.runId === b.runId && a.requestId === b.requestId;
}

/** 파일을 다시 골라야 하는 활성 본문(빈 multipart 파일 부분·빈 binary 경로). */
function needsFile(snapshot: RequestSnapshot): boolean {
  const body = snapshot.body;
  if (body.kind === "binary") return !body.path;
  return body.kind === "multipart" && body.parts.some((part) => part.kind === "file" && !part.path);
}

interface Resolved {
  variables: EnvVar[];
  sources: VariableSource[];
  missing: string[];
  blocker?: string;
}

/** 항목이 쓰는 변수를 출처 규칙(CV-F04)으로 고른다. 캡처 값은 `origin: "capture"`로 보낸다. */
function resolveVariables(
  index: number,
  ctx: { plan: RunPlan; entries: RunEntry[]; steps: RunStep[]; runId: string },
  environment: Pair[],
  session: Session,
): Resolved {
  const out: Resolved = { variables: [], sources: [], missing: [] };
  for (const use of ctx.plan.uses[index]!) {
    const producer = latestProducer(ctx.plan, use.name, index);
    const current = session.get(use.name);
    if (producer !== undefined) {
      const from = { runId: ctx.runId, requestId: ctx.entries[producer]!.id };
      if (ctx.steps[producer]?.status !== "passed" || !current || !sameOrigin(current.from, from)) {
        out.blocker ??= ctx.entries[producer]!.name;
        continue;
      }
    }
    if (current) {
      out.variables.push({ name: use.name, value: current.value, sensitive: true, origin: "capture" });
      out.sources.push({ name: use.name, from: current.from });
      continue;
    }
    const env = session.isDiscarded(use.name) ? undefined : environment.findLast((pair) => pair.name === use.name);
    if (env) {
      out.variables.push({
        name: env.name,
        value: env.value,
        sensitive: envSensitive(env.name, env.value),
        origin: "environment",
      });
      out.sources.push({ name: use.name, from: "environment" });
    } else if (use.required) out.missing.push(use.name);
  }
  return out;
}

function skipReason(entry: RunEntry, snapshot: RequestSnapshot): string | null {
  if (entry.reviewNeeded) return "비밀 검토가 필요합니다";
  if (needsFile(snapshot)) return "파일을 다시 선택해야 합니다";
  const masked = (entry.masked ?? []).flatMap((path) => (path.startsWith("request.") ? [path.slice(8)] : []));
  if (masked.length && stillMasked(toStored(entry.draft), masked).length) return "가린 값을 다시 입력해 주세요";
  return null;
}

async function execute(
  entry: RunEntry,
  snapshot: RequestSnapshot,
  variables: EnvVar[],
  step: RunStep,
  session: Session,
  deps: RunDeps,
  signal: AbortSignal,
): Promise<void> {
  try {
    const response = await deps.send(snapshot, variables, signal);
    if (signal.aborted) throw new Error("cancelled");
    step.httpStatus = response.status;
    step.durationMs = response.duration_ms;
    step.assertions = await evaluateAssertions(entry.draft.assertions, response, signal);
    if (signal.aborted) throw new Error("cancelled");
    const captured = applyCaptures(entry.draft.captures, response);
    if (step.assertions.some((result) => !result.passed)) {
      step.status = "failed";
    } else if (captured.errors.length || captured.missing.length) {
      step.status = "error";
      step.message = captured.missing.length
        ? `값을 캡처하지 못했습니다: ${captured.missing.join(", ")}`
        : "일부 값을 캡처하지 못했습니다";
    } else {
      // 통과한 요청의 캡처만 세션에 넣는다. 실패한 요청은 이전 값을 지우지도 덮지도 않는다.
      for (const [name, value] of captured.values) session.set(name, value, { runId: deps.runId, requestId: entry.id });
      step.captured = [...captured.values.keys()];
      step.status = "passed";
    }
  } catch {
    step.status = "error";
    step.captured = [];
    step.message = signal.aborted ? "요청이 취소되었습니다" : "요청 전송 또는 값 캡처에 실패했습니다";
  }
}

export async function runCollection(
  entries: RunEntry[],
  environment: Pair[],
  session: Session,
  options: RunOptions,
  deps: RunDeps,
  signal: AbortSignal,
  onStep: (step: RunStep) => void,
): Promise<RunSummary> {
  const delay = options.delayMs;
  if (entries.length > MAX_RUN_ENTRIES || !Number.isFinite(delay) || delay < 0 || delay > MAX_RUN_DELAY_MS)
    throw new Error(LIMIT_ERROR);
  const snapshots = entries.map((entry) => toSnapshot(entry.draft));
  const plan = planRun(entries.map((entry, i) => ({ produces: entry.produces, snapshot: snapshots[i]! })));
  const summary: RunSummary = {
    runId: deps.runId,
    steps: [],
    passed: 0,
    failed: 0,
    errors: 0,
    skipped: 0,
    cancelled: false,
  };
  const ctx = { plan, entries, steps: summary.steps, runId: deps.runId };
  let stopped = false;
  for (let index = 0; index < entries.length; index++) {
    const entry = entries[index]!;
    const snapshot = snapshots[index]!;
    const step: RunStep = {
      entryId: entry.id,
      name: entry.name,
      status: "skipped",
      httpStatus: null,
      durationMs: null,
      assertions: [],
      captured: [],
      sources: [],
      message: "",
    };
    const resolved = resolveVariables(index, ctx, environment, session);
    step.sources = resolved.sources;
    const reason = resolved.blocker
      ? `출처 요청 실패: ${resolved.blocker}`
      : signal.aborted
        ? "실행을 중지했습니다"
        : stopped
          ? "앞선 실패로 건너뛰었습니다"
          : skipReason(entry, snapshot);
    if (reason) step.message = reason;
    else if (resolved.missing.length) {
      step.status = "error";
      step.message = `변수 없음: ${resolved.missing.join(", ")}`;
    } else await execute(entry, snapshot, resolved.variables, step, session, deps, signal);
    summary.steps.push(step);
    if (step.status === "error") summary.errors++;
    else summary[step.status]++;
    onStep(step);
    if (options.stopOnFailure && (step.status === "failed" || step.status === "error" || resolved.blocker))
      stopped = true;
    if (index + 1 < entries.length && !stopped && !signal.aborted && delay > 0) {
      try {
        await deps.sleep(delay, signal);
      } catch {
        stopped = true;
      }
    }
  }
  summary.cancelled = signal.aborted;
  return summary;
}
