import type { ApiResponse, RequestTemplate } from "../types";
import type { CollectionEntry } from "./collections";
import type { EnvVariable } from "./environments";
import { evaluateAssertions, type AssertionResult } from "./assertions";
import { applyCaptures, VARIABLE_NAME, type Capture } from "./captures";
export interface RunOptions {
  stopOnFailure: boolean;
  delayMs: number;
}
export interface RunDeps {
  send(
    request: RequestTemplate,
    variables: EnvVariable[],
    signal: AbortSignal,
    captures?: Capture[],
  ): Promise<ApiResponse>;
  nativeCaptures?: boolean;
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
export interface CaptureAccess {
  reveal(reference: string): Promise<string>;
  discard(references: string[]): Promise<void>;
  restore(references: string[]): Promise<void>;
}
export class SessionVariables {
  private revision = 0;
  private values = new Map<string, { plain: string | null; sealed: string | null; reference?: string }>();
  constructor(private access?: CaptureAccess) {}
  get version(): number {
    return this.revision;
  }
  setNative(value: { name: string; value: string; reference: string }): void {
    if (!VARIABLE_NAME.test(value.name) || !value.value || !value.reference)
      throw new Error("캡처 값이 올바르지 않습니다");
    this.revision++;
    this.values.set(value.name, { plain: null, sealed: value.value, reference: value.reference });
    this.missing.delete(value.name);
  }
  async reveal(name: string): Promise<string> {
    const value = this.values.get(name),
      revision = this.revision;
    if (!value) throw new Error("캡처 값이 없습니다");
    if (value.plain !== null) return value.plain;
    if (!value.reference || !this.access) throw new Error("캡처 값을 표시할 수 없습니다");
    const plain = await this.access.reveal(value.reference);
    if (this.revision !== revision || this.values.get(name) !== value) throw new Error("캡처 값이 바뀌었습니다");
    return plain;
  }
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
    const references = [...this.values]
      .filter(([name]) => !names || names.includes(name))
      .flatMap(([, value]) => (value.reference ? [value.reference] : []));
    const discarded =
      references.length && this.access
        ? this.access.discard(references).then(
            () => true,
            () => false,
          )
        : null;
    for (const name of names ?? [...this.values.keys()]) this.delete(name);
    const revision = this.revision;
    return async () => {
      if (this.revision !== revision) throw new Error("그 사이 바뀐 내용이 있어 되돌리지 않았습니다.");
      if (discarded && this.access) {
        if (!(await discarded)) throw new Error("캡처 상태를 확인하지 못했습니다.");
        if (this.revision !== revision) throw new Error("그 사이 바뀐 내용이 있어 되돌리지 않았습니다.");
        await this.access.restore(references);
        if (this.revision !== revision) {
          await this.access.discard(references);
          throw new Error("그 사이 바뀐 내용이 있어 되돌리지 않았습니다.");
        }
      }
      this.values = values;
      this.missing = missing;
      this.revision++;
    };
  }
  forSend(): EnvVariable[] {
    return [...this.values].map(([key, value]) => ({
      key,
      value: value.sealed ?? value.plain ?? "",
      secret: value.sealed !== null,
    }));
  }
  entries(): { name: string; plain: string | null; reference?: string }[] {
    return [...this.values].map(([name, value]) => ({
      name,
      plain: value.plain,
      ...(value.reference ? { reference: value.reference } : {}),
    }));
  }
  /** Failed/removed captures must not fall back to an older environment value. */
  merge(environment: EnvVariable[]): EnvVariable[] {
    return [
      ...environment.filter((variable) => !this.missing.has(variable.key) && !this.values.has(variable.key)),
      ...this.forSend(),
    ];
  }
}
export async function applyResponseCaptures(
  definitions: Capture[],
  response: ApiResponse,
  session: SessionVariables,
  deps: Pick<RunDeps, "seal" | "nativeCaptures">,
  signal: AbortSignal,
  isCurrent: () => boolean = () => true,
): Promise<{ names: string[]; errors: string[] }> {
  const enabled = definitions.filter((capture) => capture.enabled && VARIABLE_NAME.test(capture.variable));
  for (const capture of enabled) session.delete(capture.variable);
  const requireCurrent = () => {
    if (signal.aborted || !isCurrent()) throw new Error("capture_cancelled");
  };
  requireCurrent();
  if (deps.nativeCaptures) {
    const result = response.captures;
    if (!result && enabled.length) throw new Error("native_capture_missing");
    const names = new Set(enabled.map((capture) => capture.variable));
    const values = result?.values.filter((value) => names.has(value.name)) ?? [];
    for (const value of values) session.setNative(value);
    return { names: values.map((value) => value.name), errors: result?.errors ?? [] };
  }
  const captured = applyCaptures(definitions, response);
  const values = [];
  for (const [name, plain] of captured.values) {
    const sealed = await deps.seal(plain);
    requireCurrent();
    if (sealed === "") throw new Error("capture_failed");
    values.push({ name, plain, sealed });
  }
  for (const value of values) session.set(value.name, value.plain, value.sealed);
  return { names: values.map((value) => value.name), errors: captured.errors };
}
export function missingVariables(request: RequestTemplate, available: Set<string>): string[] {
  const values = [
    request.url,
    ...request.headers.filter((header) => header.enabled !== false).flatMap((header) => [header.key, header.value]),
    ...request.cookies.filter((cookie) => cookie.enabled !== false).map((cookie) => cookie.value),
    ...request.params.flatMap((param) => [param.key, param.value]),
  ];
  if (["json", "form", "raw"].includes(request.body_kind)) values.push(request.body);
  if (request.body_kind === "graphql" && request.graphql)
    values.push(request.graphql.query, request.graphql.variables, request.graphql.operation_name);
  if (request.body_kind === "multipart")
    values.push(
      ...request.multipart.filter((part) => part.enabled !== false && part.kind === "text").map((part) => part.value),
    );
  if (request.auth?.kind === "oauth2" && request.auth.oauth2) {
    const config = request.auth.oauth2;
    if (config.grantType === "authorizationCode") values.push(config.authorizationUrl);
    values.push(config.tokenUrl, config.clientId, config.clientSecret, config.scopes);
  }
  if (request.auth?.kind === "basic") values.push(request.auth.username, request.auth.password);
  if (request.auth?.kind === "bearer") values.push(request.auth.token);
  if (request.auth?.kind === "apikey") values.push(request.auth.api_key, request.auth.api_value);
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
          const response = await deps.send(entry.request, variables, signal, entry.captures ?? []);
          if (signal.aborted) throw new Error("cancelled");
          step.httpStatus = response.status;
          step.durationMs = response.duration_ms;
          step.assertions = await evaluateAssertions(entry.assertions ?? [], response, signal);
          if (signal.aborted) throw new Error("cancelled");
          const captured = await applyResponseCaptures(entry.captures ?? [], response, session, deps, signal);
          step.captured = captured.names;
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
