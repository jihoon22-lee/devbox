import { convertByteEncoding } from "../tools/byteCodec";
import { convertJsonToTypescript } from "../tools/jsonTypescript";
import { convertJsonYaml } from "../tools/jsonYaml";
import { formatJwtDisplay, parseJwt } from "../tools/jwt";
import { urlComponentDecode, urlComponentEncode } from "../tools/textEncoding";
import { convertCase } from "../tools/transformers";
import { isBoundedJsonText, type SmartInputType } from "./smartDetection";

/** Maximum in-memory text passed between two local pipeline stages. */
export const PIPELINE_LIMITS = Object.freeze({
  maxInputBytes: 1_000_000,
  maxInputCodeUnits: 2_100_000,
  maxOutputBytes: 4_000_000,
  maxSteps: 8,
});

export type PipelineValueType = SmartInputType | "url-component" | "yaml" | "typescript";

/** Runtime allow-list for values crossing the workflow UI/storage boundary. */
export const PIPELINE_VALUE_TYPES: readonly PipelineValueType[] = Object.freeze([
  "text",
  "json",
  "jwt",
  "url",
  "base64",
  "base64url",
  "hex",
  "url-component",
  "yaml",
  "typescript",
]);

const PIPELINE_VALUE_TYPE_SET: ReadonlySet<string> = new Set(PIPELINE_VALUE_TYPES);

export function isPipelineValueType(value: unknown): value is PipelineValueType {
  return typeof value === "string" && PIPELINE_VALUE_TYPE_SET.has(value);
}

export interface PipelineStep {
  readonly transformerId: string;
}

export interface TransformerDescriptor {
  readonly id: string;
  readonly label: string;
  readonly inputTypes: readonly PipelineValueType[];
  readonly outputType: PipelineValueType;
  readonly description: string;
  readonly run: (input: string) => PipelineTransformResult;
}

export interface PipelineTransformResult {
  readonly output: string;
  readonly errorCode?: PipelineErrorCode;
}

export type PipelineErrorCode =
  | "invalid_input"
  | "input_too_large"
  | "too_many_steps"
  | "unknown_transformer"
  | "type_mismatch"
  | "transform_failed"
  | "output_too_large";

const PIPELINE_ERROR_CODES: ReadonlySet<string> = new Set([
  "invalid_input",
  "input_too_large",
  "too_many_steps",
  "unknown_transformer",
  "type_mismatch",
  "transform_failed",
  "output_too_large",
]);

function isPipelineErrorCode(value: unknown): value is PipelineErrorCode {
  return typeof value === "string" && PIPELINE_ERROR_CODES.has(value);
}

export interface PipelineError {
  readonly code: PipelineErrorCode;
  readonly stepIndex: number | null;
  readonly expectedTypes: readonly PipelineValueType[];
  readonly actualType: PipelineValueType | null;
}

export interface PipelineResult {
  readonly output: string;
  readonly outputType: PipelineValueType;
  readonly completedSteps: number;
  readonly error: PipelineError | null;
}

export const PIPELINE_ERROR_MESSAGES: Readonly<Record<PipelineErrorCode, string>> = Object.freeze({
  invalid_input: "변환 입력을 처리할 수 없습니다.",
  input_too_large: "파이프라인 입력은 1,000,000바이트 이하만 처리합니다.",
  too_many_steps: "파이프라인은 최대 8단계까지 구성할 수 있습니다.",
  unknown_transformer: "지원하지 않는 변환 단계입니다.",
  type_mismatch: "현재 출력 형식과 맞지 않는 변환 단계입니다.",
  transform_failed: "변환 단계를 완료하지 못했습니다.",
  output_too_large: "파이프라인 결과가 4,000,000바이트 제한을 초과합니다.",
});

const UTF8_ENCODER = new TextEncoder();

function byteLength(value: string): number {
  return UTF8_ENCODER.encode(value).byteLength;
}

function isWellFormedUnicode(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (next < 0xdc00 || next > 0xdfff) return false;
      index += 1;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) {
      return false;
    }
  }
  return true;
}

function success(output: string): PipelineTransformResult {
  return { output };
}

function failed(errorCode: PipelineErrorCode = "transform_failed"): PipelineTransformResult {
  return { output: "", errorCode };
}

function runJsonFormat(input: string, minify: boolean): PipelineTransformResult {
  if (!isBoundedJsonText(input)) return failed();
  try {
    const parsed: unknown = JSON.parse(input);
    const output = JSON.stringify(parsed, null, minify ? 0 : 2);
    return typeof output === "string" ? success(output) : failed();
  } catch {
    return failed();
  }
}

function runJsonParse(input: string): PipelineTransformResult {
  if (!isBoundedJsonText(input)) return failed();
  try {
    const parsed: unknown = JSON.parse(input);
    return typeof parsed === "undefined" ? failed() : success(JSON.stringify(parsed));
  } catch {
    return failed();
  }
}

function runByteCodec(
  input: string,
  source: "hex" | "base64" | "base64url",
  target: "utf8" | "hex" | "base64" | "base64url",
): PipelineTransformResult {
  const result = convertByteEncoding(input, source, target);
  return result.error ? failed() : success(result.output);
}

function runJsonYaml(input: string, direction: "json-to-yaml" | "yaml-to-json"): PipelineTransformResult {
  if (direction === "json-to-yaml" && !isBoundedJsonText(input)) return failed();
  const result = convertJsonYaml(input, direction);
  return result.error ? failed() : success(result.output);
}

function runJwtDecode(input: string): PipelineTransformResult {
  try {
    return success(formatJwtDisplay(parseJwt(input)));
  } catch {
    return failed();
  }
}

function runJsonTypescript(input: string): PipelineTransformResult {
  const result = convertJsonToTypescript(input, "RootObject");
  return result.error ? failed() : success(result.output);
}

function runUrl(input: string, encode: boolean): PipelineTransformResult {
  try {
    return success(encode ? urlComponentEncode(input) : urlComponentDecode(input));
  } catch {
    return failed();
  }
}

function runCase(input: string): PipelineTransformResult {
  try {
    return success(convertCase(input));
  } catch {
    return failed();
  }
}

const descriptor = (
  id: string,
  label: string,
  inputTypes: readonly PipelineValueType[],
  outputType: PipelineValueType,
  description: string,
  run: (input: string) => PipelineTransformResult,
): TransformerDescriptor => ({ id, label, inputTypes, outputType, description, run });

/**
 * Only deterministic, offline transformations are exposed here.  In
 * particular there is no shell/API receiver stage: a saved pipeline is a
 * list of these IDs and can be replayed without restoring any previous text.
 */
export const TRANSFORMERS: readonly TransformerDescriptor[] = [
  descriptor("json-format", "JSON 포매터", ["json"], "json", "읽기 쉬운 들여쓰기를 적용한 엄격한 JSON.", (input) => runJsonFormat(input, false)),
  descriptor("json-minify", "JSON 최소화", ["json"], "json", "불필요한 공백을 제거한 엄격한 JSON.", (input) => runJsonFormat(input, true)),
  descriptor("json-parse", "JSON 파싱", ["text"], "json", "JSON 단계 전에 텍스트가 엄격한 JSON인지 확인합니다.", runJsonParse),
  descriptor("json-to-yaml", "JSON → YAML", ["json"], "yaml", "엄격한 JSON 값 하나를 YAML 1.2로 변환합니다.", (input) => runJsonYaml(input, "json-to-yaml")),
  descriptor("yaml-to-json", "YAML → JSON", ["yaml"], "json", "제한된 YAML 1.2 값 하나를 JSON으로 변환합니다.", (input) => runJsonYaml(input, "yaml-to-json")),
  descriptor("json-to-typescript", "JSON → TypeScript", ["json"], "typescript", "제한된 TypeScript 선언을 추론합니다.", runJsonTypescript),
  descriptor("jwt-decode", "JWT 디코더", ["jwt"], "json", "클레임을 검증되지 않은 JSON으로 디코드하며 키를 검증하거나 저장하지 않습니다.", runJwtDecode),
  descriptor("url-encode", "URL 컴포넌트 인코딩", ["text"], "url-component", "컴포넌트 하나를 인코딩하며 URL을 조합하거나 열지 않습니다.", (input) => runUrl(input, true)),
  descriptor("url-decode", "URL 컴포넌트 디코딩", ["url", "url-component", "text"], "text", "네트워크 접근 없이 컴포넌트 하나를 디코드합니다.", (input) => runUrl(input, false)),
  descriptor("base64-encode", "Base64 인코딩", ["text"], "base64", "UTF-8 텍스트를 Base64로 인코딩합니다.", (input) => {
    const result = convertByteEncoding(input, "utf8", "base64");
    return result.error ? failed() : success(result.output);
  }),
  descriptor("base64-decode", "Base64 디코드", ["base64"], "text", "바이트가 유효한 UTF-8일 때만 Base64를 디코드합니다.", (input) => runByteCodec(input, "base64", "utf8")),
  descriptor("base64-to-hex", "Base64 → Hex", ["base64"], "hex", "Base64를 손실 없는 원시 바이트 Hex로 디코드합니다.", (input) => runByteCodec(input, "base64", "hex")),
  descriptor("base64url-encode", "Base64URL 인코딩", ["text"], "base64url", "UTF-8 텍스트를 패딩 없는 Base64URL로 인코딩합니다.", (input) => {
    const result = convertByteEncoding(input, "utf8", "base64url");
    return result.error ? failed() : success(result.output);
  }),
  descriptor("base64url-decode", "Base64URL 디코드", ["base64url"], "text", "바이트가 유효한 UTF-8일 때만 Base64URL을 디코드합니다.", (input) => runByteCodec(input, "base64url", "utf8")),
  descriptor("base64url-to-hex", "Base64URL → Hex", ["base64url"], "hex", "Base64URL을 손실 없는 원시 바이트 Hex로 디코드합니다.", (input) => runByteCodec(input, "base64url", "hex")),
  descriptor("hex-encode", "Hex 인코딩", ["text"], "hex", "UTF-8 텍스트를 원시 바이트 Hex로 인코딩합니다.", (input) => {
    const result = convertByteEncoding(input, "utf8", "hex");
    return result.error ? failed() : success(result.output);
  }),
  descriptor("hex-decode", "Hex 디코드", ["hex"], "text", "바이트가 유효한 UTF-8일 때만 Hex를 디코드합니다.", (input) => runByteCodec(input, "hex", "utf8")),
  descriptor("hex-to-base64", "Hex → Base64", ["hex"], "base64", "Hex를 손실 없는 Base64 바이트로 디코드합니다.", (input) => runByteCodec(input, "hex", "base64")),
  descriptor("case", "대소문자 변환", ["text"], "text", "결정적인 텍스트 대소문자 변형을 만듭니다.", runCase),
];

export const TRANSFORMER_BY_ID: ReadonlyMap<string, TransformerDescriptor> = new Map(
  TRANSFORMERS.map((item) => [item.id, item]),
);

function errorResult(
  code: PipelineErrorCode,
  stepIndex: number | null,
  expectedTypes: readonly PipelineValueType[] = [],
  actualType: PipelineValueType | null = null,
  outputType: PipelineValueType = actualType ?? "text",
  completedSteps = 0,
): PipelineResult {
  return {
    output: "",
    outputType,
    completedSteps,
    error: { code, stepIndex, expectedTypes, actualType },
  };
}

function outputWithinBounds(output: string): boolean {
  return typeof output === "string"
    && isWellFormedUnicode(output)
    && byteLength(output) <= PIPELINE_LIMITS.maxOutputBytes;
}

function transformerIdOf(step: unknown): string | null {
  if (typeof step !== "object" || step === null || Array.isArray(step)) return null;
  const transformerId = (step as { transformerId?: unknown }).transformerId;
  return typeof transformerId === "string" ? transformerId : null;
}

/** Return a fixed, non-reflective error if the selected step is incompatible. */
export function pipelineCompatibility(
  currentType: PipelineValueType,
  transformerId: string,
): { compatible: boolean; descriptor: TransformerDescriptor | null } {
  const item = TRANSFORMER_BY_ID.get(transformerId) ?? null;
  return {
    descriptor: item,
    compatible: item !== null && item.inputTypes.includes(currentType),
  };
}

export function pipelineOutputType(
  initialType: PipelineValueType,
  steps: readonly PipelineStep[],
): PipelineValueType {
  if (!isPipelineValueType(initialType) || !Array.isArray(steps)) return "text";
  let current = initialType;
  for (const step of steps) {
    const transformerId = transformerIdOf(step);
    const item = transformerId === null ? null : TRANSFORMER_BY_ID.get(transformerId);
    if (!item || !item.inputTypes.includes(current)) return current;
    current = item.outputType;
  }
  return current;
}

/** Execute only an explicit, bounded list of typed local transformations. */
export function runPipeline(
  input: string,
  initialType: PipelineValueType,
  steps: readonly PipelineStep[],
): PipelineResult {
  if (
    typeof input !== "string"
    || !isWellFormedUnicode(input)
    || !isPipelineValueType(initialType)
    || !Array.isArray(steps)
  ) {
    return errorResult("invalid_input", null, [], isPipelineValueType(initialType) ? initialType : null);
  }
  if (
    input.length > PIPELINE_LIMITS.maxInputCodeUnits
    || byteLength(input) > PIPELINE_LIMITS.maxInputBytes
  ) {
    return errorResult("input_too_large", null, [], initialType);
  }
  if (steps.length > PIPELINE_LIMITS.maxSteps) {
    return errorResult("too_many_steps", null, [], initialType);
  }

  let value = input;
  let currentType = initialType;
  for (let index = 0; index < steps.length; index += 1) {
    const step = steps[index]!;
    const transformerId = transformerIdOf(step);
    const item = transformerId === null ? null : TRANSFORMER_BY_ID.get(transformerId);
    if (!item) return errorResult("unknown_transformer", index, [], currentType, currentType, index);
    if (!item.inputTypes.includes(currentType)) {
      return errorResult("type_mismatch", index, item.inputTypes, currentType, currentType, index);
    }

    let result: unknown;
    try {
      result = item.run(value);
    } catch {
      return errorResult("transform_failed", index, item.inputTypes, currentType, currentType, index);
    }
    if (typeof result !== "object" || result === null || Array.isArray(result)) {
      return errorResult("transform_failed", index, item.inputTypes, currentType, currentType, index);
    }
    const rawResult = result as { output?: unknown; errorCode?: unknown };
    if (typeof rawResult.output !== "string") {
      return errorResult("transform_failed", index, item.inputTypes, currentType, currentType, index);
    }
    if (rawResult.errorCode !== undefined) {
      if (!isPipelineErrorCode(rawResult.errorCode)) {
        return errorResult("transform_failed", index, item.inputTypes, currentType, currentType, index);
      }
      return errorResult(rawResult.errorCode, index, item.inputTypes, currentType, currentType, index);
    }
    if (!outputWithinBounds(rawResult.output)) {
      return errorResult("output_too_large", index, item.inputTypes, currentType, currentType, index);
    }
    value = rawResult.output;
    currentType = item.outputType;
  }

  return { output: value, outputType: currentType, completedSteps: steps.length, error: null };
}

export function pipelineErrorMessage(error: PipelineError | null): string | null {
  return error ? PIPELINE_ERROR_MESSAGES[error.code] : null;
}
