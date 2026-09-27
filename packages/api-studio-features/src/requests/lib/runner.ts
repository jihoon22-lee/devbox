import type { ApiResponse, RequestTemplate } from "../types";
import type { CollectionEntry } from "./collections";
import type { EnvVariable } from "./environments";
import { evaluateAssertions, type AssertionResult } from "./assertions";
import { applyCaptures, VARIABLE_NAME } from "./captures";
export interface RunOptions {
  stopOnFailure: boolean;
  delayMs: number;
}
export interface RunDeps {
  send(request: RequestTemplate, variables: EnvVariable[], signal: AbortSignal): Promise<ApiResponse>;
  seal(value: string): Promise<string | null>;
  sleep(ms: number, signal: AbortSignal): Promise<void>;
}
export interface RunStep {
  entryId: string;
  name: string;
  status: "passed" | "failed" | "error" | "skipped";
  httpStatus: number | null;
  durationMs: number | null;
  assertions: AssertionResult[];
  captured: string[];
  message: string;
}
export interface RunSummary {
  steps: RunStep[];
  passed: number;
  failed: number;
  errors: number;
  skipped: number;
  cancelled: boolean;
}
export class SessionVariables {
  private revision = 0;
  private values = new Map<string, { plain: string; sealed: string | null }>();
  private missing = new Set<string>();
  set(name: string, plain: string, sealed: string | null): void {
    if (!VARIABLE_NAME.test(name) || new TextEncoder().encode(plain).length > 64 * 1024)
      throw new Error("세션 변수가 올바르지 않습니다");
    this.revision++;
    this.values.set(name, { plain, sealed });
    this.missing.delete(name);
  }
  delete(name: string): void {
    this.revision++;
    this.values.delete(name);
    this.missing.add(name);
  }
  clear(): void {
    this.revision++;
    this.values.clear();
    this.missing.clear();
  }
  discard(names?: string[]): () => Promise<void> {
    const values = new Map(this.values),
      missing = new Set(this.missing);
    if (names) for (const name of names) this.delete(name);
    else this.clear();
    const revision = this.revision;
    return async () => {
      if (this.revision !== revision) throw new Error("그 사이 바뀐 내용이 있어 되돌리지 않았습니다.");
      this.values = values;
      this.missing = missing;
      this.revision++;
    };
  }
  forSend(): EnvVariable[] {
    return [...this.values].map(([key, value]) => ({
      key,
      value: value.sealed ?? value.plain,
      secret: value.sealed !== null,
    }));
  }
  entries(): { name: string; plain: string }[] {
    return [...this.values].map(([name, value]) => ({ name, plain: value.plain }));
  }
  /** Failed/removed captures must not fall back to an older environment value. */
  merge(environment: EnvVariable[]): EnvVariable[] {
    return [
      ...environment.filter((variable) => !this.missing.has(variable.key) && !this.values.has(variable.key)),
      ...this.forSend(),
    ];
  }
}
export function missingVariables(request: RequestTemplate, available: Set<string>): string[] {
  const values = [
    request.url,
    ...request.headers.filter((header) => header.enabled !== false).flatMap((header) => [header.key, header.value]),
    ...request.cookies.filter((cookie) => cookie.enabled !== false).map((cookie) => cookie.value),
    ...request.params.flatMap((param) => [param.key, param.value]),
  ];
  if (request.body_kind !== "multipart") values.push(request.body);
  if (request.body_kind === "graphql" && request.graphql)
    values.push(request.graphql.query, request.graphql.variables, request.graphql.operation_name);
  if (request.body_kind === "multipart")
    values.push(
      ...request.multipart.filter((part) => part.enabled !== false && part.kind === "text").map((part) => part.value),
    );
  if (request.auth)
    values.push(
      request.auth.username,
      request.auth.password,
      request.auth.token,
      request.auth.api_key,
      request.auth.api_value,
    );
  const missing = new Set<string>();
  for (const value of values)
    for (const match of value.matchAll(/\{\{\s*([A-Za-z0-9_.-]+)\s*\}\}|\$\{\s*([A-Za-z0-9_.-]+)\s*\}/g)) {
      const name = match[1] ?? match[2];
      if (!available.has(name)) missing.add(name);
    }
  return [...missing];
}
export async function runCollection(
  entries: CollectionEntry[],
  environment: EnvVariable[],
  session: SessionVariables,
  options: RunOptions,
  deps: RunDeps,
  signal: AbortSignal,
  onStep: (step: RunStep) => void,
): Promise<RunSummary> {
  if (entries.length > 200 || !Number.isFinite(options.delayMs) || options.delayMs < 0 || options.delayMs > 5000)
    throw new Error("러너 한도를 넘었습니다. 요청 200개, 대기 0–5000ms까지 사용할 수 있습니다.");
  const summary: RunSummary = { steps: [], passed: 0, failed: 0, errors: 0, skipped: 0, cancelled: false };
  let stopped = false;
  for (let index = 0; index < entries.length; index++) {
    const entry = entries[index];
    const step: RunStep = {
      entryId: entry.id,
      name: entry.name,
      status: "skipped",
      httpStatus: null,
      durationMs: null,
      assertions: [],
      captured: [],
      message: "",
    };
    const clearCaptures = () => {
      for (const capture of entry.captures ?? [])
        if (capture.enabled && VARIABLE_NAME.test(capture.variable)) session.delete(capture.variable);
    };
    if (signal.aborted || stopped) step.message = signal.aborted ? "실행을 중지했습니다" : "앞선 실패로 건너뛰었습니다";
    else if (entry.requiresSecretReview || entry.request.requiresSecretReview) step.message = "비밀 검토가 필요합니다";
    else if (
      entry.request.body_kind === "multipart" &&
      entry.request.multipart.some((part) => part.enabled !== false && part.kind === "file" && !part.file_path)
    )
      step.message = "파일을 다시 선택해야 합니다";
    else {
      const variables = session.merge(environment);
      const missing = missingVariables(
        entry.request,
        new Set(
          variables.filter((variable) => !variable.secret || variable.value !== "").map((variable) => variable.key),
        ),
      );
      if (missing.length) {
        step.status = "error";
        step.message = `변수 없음: ${missing.join(", ")}`;
        clearCaptures();
      } else {
        try {
          const response = await deps.send(entry.request, variables, signal);
          if (signal.aborted) throw new Error("cancelled");
          step.httpStatus = response.status;
          step.durationMs = response.duration_ms;
          step.assertions = evaluateAssertions(entry.assertions ?? [], response);
          const captured = applyCaptures(entry.captures ?? [], response);
          clearCaptures();
          for (const [name, plain] of captured.values) {
            const sealed = await deps.seal(plain);
            if (signal.aborted || sealed === "") throw new Error("capture_failed");
            session.set(name, plain, sealed);
            step.captured.push(name);
          }
          step.status = step.assertions.some((result) => !result.passed)
            ? "failed"
            : captured.errors.length
              ? "error"
              : "passed";
          if (captured.errors.length) step.message = "일부 값을 캡처하지 못했습니다";
        } catch {
          clearCaptures();
          step.captured = [];
          step.status = "error";
          step.message = signal.aborted ? "요청이 취소되었습니다" : "요청 전송 또는 값 캡처에 실패했습니다";
        }
      }
    }
    summary.steps.push(step);
    if (step.status === "error") summary.errors++;
    else summary[step.status]++;
    onStep(step);
    if (options.stopOnFailure && (step.status === "failed" || step.status === "error")) stopped = true;
    if (index + 1 < entries.length && !stopped && !signal.aborted && options.delayMs > 0) {
      try {
        await deps.sleep(options.delayMs, signal);
      } catch {
        stopped = true;
      }
    }
  }
  summary.cancelled = signal.aborted;
  return summary;
}
