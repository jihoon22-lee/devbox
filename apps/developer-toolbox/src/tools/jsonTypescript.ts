import { visit as visitJson, type ParseError } from "jsonc-parser";

export const MAX_JSON_TYPESCRIPT_INPUT_BYTES = 1_000_000;
export const MAX_JSON_TYPESCRIPT_OUTPUT_BYTES = 4_000_000;
export const MAX_JSON_TYPESCRIPT_DEPTH = 64;
export const MAX_JSON_TYPESCRIPT_NODES = 100_000;
export const MAX_ROOT_TYPE_NAME_LENGTH = 80;

export interface JsonTypescriptError {
  code: string;
  message: string;
  line: number | null;
  column: number | null;
}

export interface JsonTypescriptResult {
  output: string;
  error: JsonTypescriptError | null;
}

type PrimitiveName = "boolean" | "null" | "number" | "string";

type TypeNode =
  | { kind: "array"; element: TypeNode }
  | { kind: "never" }
  | { kind: "object"; properties: ReadonlyMap<string, PropertyNode> }
  | { kind: "primitive"; name: PrimitiveName }
  | { kind: "union"; members: readonly TypeNode[] };

interface PropertyNode {
  optional: boolean;
  type: TypeNode;
}

const NEVER: TypeNode = { kind: "never" };

const RESERVED_TYPE_NAMES = new Set([
  "abstract", "accessor", "any", "arguments", "as", "asserts", "async", "await", "bigint",
  "boolean", "break", "case",
  "catch", "class", "const", "constructor", "continue", "debugger", "declare", "default",
  "delete", "do", "else", "enum", "export", "extends", "false", "finally", "for", "from",
  "function", "get", "global", "if", "implements", "import", "in", "infer", "instanceof",
  "interface", "intrinsic", "is", "keyof", "let", "module", "namespace", "never", "new", "null",
  "number", "object", "of", "out", "override", "package", "private", "protected", "public", "readonly",
  "require", "return", "satisfies", "set", "static", "string", "super", "switch", "symbol", "this",
  "throw", "true", "try", "type", "typeof", "undefined", "unique", "unknown", "using",
  "var", "void", "while", "with", "yield", "eval",
]);

function compareText(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
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

function error(
  code: string,
  message: string,
  line: number | null = null,
  column: number | null = null,
): JsonTypescriptResult {
  return { output: "", error: { code, message, line, column } };
}

function validateRootTypeName(name: string): JsonTypescriptResult | null {
  if (name.length === 0) {
    return error("EMPTY_ROOT_TYPE_NAME", "root type 이름을 입력해야 합니다.");
  }
  if (name.length > MAX_ROOT_TYPE_NAME_LENGTH) {
    return error(
      "ROOT_TYPE_NAME_TOO_LONG",
      `root type 이름은 ${MAX_ROOT_TYPE_NAME_LENGTH}자 이하여야 합니다.`,
    );
  }
  if (!/^[A-Za-z_$][A-Za-z0-9_$]*$/u.test(name)) {
    return error(
      "INVALID_ROOT_TYPE_NAME",
      "root type 이름은 영문자, 숫자, _, $만 사용하고 숫자로 시작할 수 없습니다.",
    );
  }
  if (RESERVED_TYPE_NAMES.has(name)) {
    return error("RESERVED_ROOT_TYPE_NAME", "TypeScript 예약어는 root type 이름으로 사용할 수 없습니다.");
  }
  return null;
}

function typeKey(node: TypeNode): string {
  switch (node.kind) {
    case "never":
      return "0:never";
    case "primitive":
      return `1:${node.name}`;
    case "array":
      return `2:${typeKey(node.element)}`;
    case "object":
      return `3:${[...node.properties.entries()]
        .map(([name, property]) => `${JSON.stringify(name)}${property.optional ? "?" : ""}:${typeKey(property.type)}`)
        .join(",")}`;
    case "union":
      return `4:${node.members.map(typeKey).join("|")}`;
  }
}

function mergeObjectGroup(
  objects: readonly Extract<TypeNode, { kind: "object" }>[],
): Extract<TypeNode, { kind: "object" }> {
  const grouped = new Map<string, { optional: boolean; present: number; types: TypeNode[] }>();

  for (const object of objects) {
    for (const [name, property] of object.properties) {
      const group = grouped.get(name) ?? { optional: false, present: 0, types: [] };
      group.optional ||= property.optional;
      group.present += 1;
      group.types.push(property.type);
      grouped.set(name, group);
    }
  }

  const properties = new Map<string, PropertyNode>();
  for (const name of [...grouped.keys()].sort(compareText)) {
    const group = grouped.get(name)!;
    properties.set(name, {
      optional: group.optional || group.present < objects.length,
      type: normalizeUnion(group.types),
    });
  }

  return { kind: "object", properties };
}

function normalizeUnion(nodes: readonly TypeNode[]): TypeNode {
  const flattened = nodes.flatMap((node) => node.kind === "union" ? node.members : [node]);
  const meaningful = flattened.filter((node) => node.kind !== "never");
  if (meaningful.length === 0) return NEVER;

  const objects = meaningful.filter((node): node is Extract<TypeNode, { kind: "object" }> => node.kind === "object");
  const arrays = meaningful.filter((node): node is Extract<TypeNode, { kind: "array" }> => node.kind === "array");
  const others = meaningful.filter((node) => node.kind !== "object" && node.kind !== "array");
  const merged: TypeNode[] = [...others];

  if (arrays.length > 0) {
    merged.push({ kind: "array", element: normalizeUnion(arrays.map((node) => node.element)) });
  }
  if (objects.length > 0) {
    merged.push(mergeObjectGroup(objects));
  }

  const unique = new Map<string, TypeNode>();
  for (const node of merged) unique.set(typeKey(node), node);
  const members = [...unique.entries()]
    .sort(([left], [right]) => compareText(left, right))
    .map(([, node]) => node);

  return members.length === 1 ? members[0]! : { kind: "union", members };
}

class InferenceLimitError extends Error {
  constructor(readonly code: "INPUT_TOO_DEEP" | "INPUT_TOO_COMPLEX") {
    super(code);
  }
}

function inferType(value: unknown, depth: number, visited: { count: number }): TypeNode {
  if (depth > MAX_JSON_TYPESCRIPT_DEPTH) throw new InferenceLimitError("INPUT_TOO_DEEP");
  visited.count += 1;
  if (visited.count > MAX_JSON_TYPESCRIPT_NODES) throw new InferenceLimitError("INPUT_TOO_COMPLEX");

  if (value === null) return { kind: "primitive", name: "null" };
  const valueType = typeof value;
  if (valueType === "string" || valueType === "number" || valueType === "boolean") {
    return { kind: "primitive", name: valueType };
  }
  if (Array.isArray(value)) {
    const element = normalizeUnion(value.map((item) => inferType(item, depth + 1, visited)));
    return { kind: "array", element };
  }

  const record = value as Record<string, unknown>;
  const properties = new Map<string, PropertyNode>();
  for (const name of Object.keys(record).sort(compareText)) {
    properties.set(name, {
      optional: false,
      type: inferType(record[name], depth + 1, visited),
    });
  }
  return { kind: "object", properties };
}

function propertyName(name: string): string {
  return /^[A-Za-z_$][A-Za-z0-9_$]*$/u.test(name) ? name : JSON.stringify(name);
}

function renderType(node: TypeNode, indentation: number): string {
  switch (node.kind) {
    case "never":
      return "unknown";
    case "primitive":
      return node.name;
    case "union":
      return node.members.map((member) => renderType(member, indentation)).join(" | ");
    case "array":
      return `Array<${renderType(node.element, indentation)}>`;
    case "object": {
      if (node.properties.size === 0) return "Record<string, never>";
      const padding = "  ".repeat(indentation);
      const childPadding = "  ".repeat(indentation + 1);
      const lines = [...node.properties.entries()].map(([name, property]) => (
        `${childPadding}${propertyName(name)}${property.optional ? "?" : ""}: ${renderType(property.type, indentation + 1)};`
      ));
      return `{\n${lines.join("\n")}\n${padding}}`;
    }
  }
}

function renderOutput(rootTypeName: string, root: TypeNode): string {
  if (root.kind === "object" && root.properties.size > 0) {
    const body = renderType(root, 0);
    return `export interface ${rootTypeName} ${body}\n`;
  }
  return `export type ${rootTypeName} = ${renderType(root, 0)};\n`;
}

export function convertJsonToTypescript(input: string, rootTypeName: string): JsonTypescriptResult {
  const nameError = validateRootTypeName(rootTypeName);
  if (nameError) return nameError;
  if (input.trim().length === 0) return { output: "", error: null };
  if (byteLength(input) > MAX_JSON_TYPESCRIPT_INPUT_BYTES) {
    return error("INPUT_TOO_LARGE", "JSON 입력이 1,000,000바이트 제한을 초과합니다.");
  }

  const errors: ParseError[] = [];
  try {
    visitJson(input, {
      onError: (parseError, offset, length) => errors.push({ error: parseError, offset, length }),
    }, {
      allowEmptyContent: false,
      allowTrailingComma: false,
      disallowComments: true,
    });
  } catch {
    return error("JSON_PARSE_FAILED", "JSON 구조가 안전한 처리 범위를 초과했습니다.");
  }

  const firstError = errors[0];
  if (firstError) {
    const location = locationAt(input, firstError.offset);
    return error("INVALID_JSON", "JSON 구문을 해석할 수 없습니다.", location.line, location.column);
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(input) as unknown;
  } catch {
    return error("INVALID_JSON", "JSON 구문을 해석할 수 없습니다.");
  }

  try {
    const output = renderOutput(rootTypeName, inferType(parsed, 0, { count: 0 }));
    if (byteLength(output) > MAX_JSON_TYPESCRIPT_OUTPUT_BYTES) {
      return error("OUTPUT_TOO_LARGE", "TypeScript 결과가 4,000,000바이트 제한을 초과합니다.");
    }
    return { output, error: null };
  } catch (caught) {
    if (caught instanceof InferenceLimitError && caught.code === "INPUT_TOO_DEEP") {
      return error("INPUT_TOO_DEEP", `JSON 중첩은 최대 ${MAX_JSON_TYPESCRIPT_DEPTH}단계까지 지원합니다.`);
    }
    if (caught instanceof InferenceLimitError) {
      return error("INPUT_TOO_COMPLEX", `JSON 값은 최대 ${MAX_JSON_TYPESCRIPT_NODES.toLocaleString("en-US")}개까지 처리합니다.`);
    }
    return error("TYPE_INFERENCE_FAILED", "JSON 구조에서 TypeScript type을 생성하지 못했습니다.");
  }
}
