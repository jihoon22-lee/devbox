import { visit as visitJson, type ParseError } from "jsonc-parser";
import { LineCounter, parseDocument, stringify } from "yaml";

export type JsonYamlDirection = "json-to-yaml" | "yaml-to-json";

export const MAX_JSON_YAML_INPUT_BYTES = 1_000_000;
export const MAX_JSON_YAML_OUTPUT_BYTES = 4_000_000;
const MAX_YAML_ALIAS_COUNT = 50;

export interface ConversionError {
  code: string;
  message: string;
  line: number | null;
  column: number | null;
}

export interface ConversionResult {
  output: string;
  error: ConversionError | null;
}

function byteLength(value: string): number {
  return new TextEncoder().encode(value).length;
}

function locationAt(input: string, offset: number): { line: number; column: number } {
  const bounded = Math.min(Math.max(0, offset), input.length);
  const before = input.slice(0, bounded);
  const lines = before.split("\n");
  return { line: lines.length, column: (lines[lines.length - 1]?.length ?? 0) + 1 };
}

function boundedOutput(output: string): ConversionResult {
  if (byteLength(output) > MAX_JSON_YAML_OUTPUT_BYTES) {
    return {
      output: "",
      error: {
        code: "OUTPUT_TOO_LARGE",
        message: "변환 결과가 4,000,000바이트 제한을 초과합니다.",
        line: null,
        column: null,
      },
    };
  }
  return { output, error: null };
}

function jsonToYaml(input: string): ConversionResult {
  const errors: ParseError[] = [];
  let unsupportedNumberOffset: number | null = null;
  try {
    visitJson(input, {
      onError: (error, offset, length) => errors.push({ error, offset, length }),
      onLiteralValue: (value, offset) => {
        if (
          typeof value === "number"
          && (!Number.isFinite(value) || (Number.isInteger(value) && !Number.isSafeInteger(value)))
        ) {
          unsupportedNumberOffset ??= offset;
        }
      },
    }, {
      allowEmptyContent: false,
      allowTrailingComma: false,
      disallowComments: true,
    });
  } catch {
    return {
      output: "",
      error: {
        code: "JSON_PARSE_FAILED",
        message: "JSON 구조가 안전한 처리 범위를 초과했습니다.",
        line: null,
        column: null,
      },
    };
  }
  const firstError = errors[0];
  if (firstError) {
    const location = locationAt(input, firstError.offset);
    return {
      output: "",
      error: {
        code: "INVALID_JSON",
        message: "JSON 구문을 해석할 수 없습니다.",
        line: location.line,
        column: location.column,
      },
    };
  }
  if (unsupportedNumberOffset !== null) {
    const location = locationAt(input, unsupportedNumberOffset);
    return {
      output: "",
      error: {
        code: "UNSUPPORTED_JSON_NUMBER",
        message: "안전하게 표현할 수 없는 JSON 숫자입니다.",
        line: location.line,
        column: location.column,
      },
    };
  }

  let value: unknown;
  try {
    value = JSON.parse(input) as unknown;
  } catch {
    return {
      output: "",
      error: {
        code: "INVALID_JSON",
        message: "JSON 구문을 해석할 수 없습니다.",
        line: null,
        column: null,
      },
    };
  }

  try {
    return boundedOutput(stringify(value, {
      aliasDuplicateObjects: false,
      indent: 2,
      lineWidth: 0,
    }));
  } catch {
    return {
      output: "",
      error: {
        code: "YAML_SERIALIZE_FAILED",
        message: "JSON 값을 YAML로 직렬화할 수 없습니다.",
        line: null,
        column: null,
      },
    };
  }
}

const YAML_ERROR_MESSAGES: Readonly<Record<string, string>> = {
  BAD_INDENT: "YAML 들여쓰기가 올바르지 않습니다.",
  DUPLICATE_KEY: "YAML mapping key가 중복되었습니다.",
  MULTIPLE_DOCS: "한 번에 YAML 문서 하나만 변환할 수 있습니다.",
  RESOURCE_EXHAUSTION: "YAML 중첩 구조가 안전한 처리 범위를 초과했습니다.",
  TAB_AS_INDENT: "YAML 들여쓰기에는 탭 대신 공백을 사용해야 합니다.",
  TAG_RESOLVE_FAILED: "YAML tag를 JSON 값으로 안전하게 해석할 수 없습니다.",
};

type YamlValueIssue = "UNSUPPORTED_YAML_GRAPH" | "UNSUPPORTED_YAML_VALUE";

function normalizeYamlValue(value: unknown): { value: unknown; issue: YamlValueIssue | null } {
  const visiting = new WeakSet<object>();
  const finished = new WeakSet<object>();

  const visit = (
    current: unknown,
    parent?: Record<string, unknown> | unknown[],
    key?: string | number,
  ): YamlValueIssue | null => {
    if (typeof current === "bigint") {
      if (current > BigInt(Number.MAX_SAFE_INTEGER) || current < BigInt(Number.MIN_SAFE_INTEGER)) {
        return "UNSUPPORTED_YAML_VALUE";
      }
      if (Array.isArray(parent) && typeof key === "number") parent[key] = Number(current);
      else if (parent !== undefined && key !== undefined) {
        (parent as Record<string, unknown>)[String(key)] = Number(current);
      }
      return null;
    }
    if (typeof current === "number") {
      return Number.isFinite(current) ? null : "UNSUPPORTED_YAML_VALUE";
    }
    if (current === null || typeof current !== "object") return null;
    if (visiting.has(current)) return "UNSUPPORTED_YAML_GRAPH";
    if (finished.has(current)) return null;
    if (!Array.isArray(current)) {
      const prototype = Object.getPrototypeOf(current);
      if (prototype !== Object.prototype && prototype !== null) return "UNSUPPORTED_YAML_VALUE";
    }

    visiting.add(current);
    if (Array.isArray(current)) {
      for (let index = 0; index < current.length; index += 1) {
        const issue = visit(current[index], current, index);
        if (issue) return issue;
      }
    } else {
      const record = current as Record<string, unknown>;
      for (const property of Object.keys(record)) {
        const issue = visit(record[property], record, property);
        if (issue) return issue;
      }
    }
    visiting.delete(current);
    finished.add(current);
    return null;
  };

  const root: unknown[] = [value];
  const issue = visit(root[0], root, 0);
  return { value: root[0], issue };
}

function yamlToJson(input: string): ConversionResult {
  const lineCounter = new LineCounter();
  let document: ReturnType<typeof parseDocument>;
  try {
    document = parseDocument(input, {
      lineCounter,
      intAsBigInt: true,
      merge: false,
      prettyErrors: true,
      resolveKnownTags: false,
      strict: true,
      stringKeys: true,
      uniqueKeys: true,
      version: "1.2",
    });
  } catch {
    return {
      output: "",
      error: {
        code: "YAML_PARSE_FAILED",
        message: "YAML 구문을 안전하게 해석할 수 없습니다.",
        line: null,
        column: null,
      },
    };
  }

  const firstIssue = document.errors[0] ?? document.warnings[0];
  if (firstIssue) {
    const position = firstIssue.linePos?.[0]
      ?? (firstIssue.pos?.[0] !== undefined ? lineCounter.linePos(firstIssue.pos[0]) : undefined);
    return {
      output: "",
      error: {
        code: firstIssue.code || "INVALID_YAML",
        message: YAML_ERROR_MESSAGES[firstIssue.code] ?? "YAML 구문을 해석할 수 없습니다.",
        line: position?.line ?? null,
        column: position?.col ?? null,
      },
    };
  }

  try {
    const parsedValue = document.toJS({ maxAliasCount: MAX_YAML_ALIAS_COUNT });
    const normalized = normalizeYamlValue(parsedValue);
    if (normalized.issue) {
      return {
        output: "",
        error: normalized.issue === "UNSUPPORTED_YAML_GRAPH"
          ? {
              code: normalized.issue,
              message: "순환하거나 과도하게 확장되는 YAML anchor/alias는 JSON으로 변환할 수 없습니다.",
              line: null,
              column: null,
            }
          : {
              code: normalized.issue,
              message: "JSON에서 안전하게 표현할 수 없는 YAML 값입니다.",
              line: null,
              column: null,
            },
      };
    }
    const output = JSON.stringify(normalized.value, null, 2);
    if (output === undefined) throw new Error("not JSON representable");
    return boundedOutput(output);
  } catch {
    return {
      output: "",
      error: {
        code: "UNSUPPORTED_YAML_GRAPH",
        message: "순환하거나 과도하게 확장되는 YAML anchor/alias는 JSON으로 변환할 수 없습니다.",
        line: null,
        column: null,
      },
    };
  }
}

export function convertJsonYaml(input: string, direction: JsonYamlDirection): ConversionResult {
  if (input.trim().length === 0) return { output: "", error: null };
  if (byteLength(input) > MAX_JSON_YAML_INPUT_BYTES) {
    return {
      output: "",
      error: {
        code: "INPUT_TOO_LARGE",
        message: "입력은 최대 1,000,000바이트까지 변환할 수 있습니다.",
        line: null,
        column: null,
      },
    };
  }
  return direction === "json-to-yaml" ? jsonToYaml(input) : yamlToJson(input);
}
