import { isExactVariableReference } from "../references";
import { isGraphqlCredentialName } from "./build";

/** Replace GraphQL string literals before persistence/display redaction. */
export function maskGraphqlQueryLiterals(query: string): string {
  let output = "";
  let index = 0;
  while (index < query.length) {
    if (query[index] !== '"') {
      output += query[index];
      index += 1;
      continue;
    }
    const block = query.slice(index, index + 3) === '"""';
    const opening = block ? 3 : 1;
    const closing = block ? '"""' : '"';
    const contentStart = index + opening;
    let cursor = contentStart;
    let closed = false;
    while (cursor < query.length) {
      if (query.slice(cursor, cursor + closing.length) === closing) {
        closed = true;
        break;
      }
      // block string의 `\"""`는 닫는 따옴표가 아니다(v0.9.0에서 이 뒤 글자가 가려지지 않던 결함).
      if (block && query.startsWith('\\"""', cursor)) cursor += 4;
      else if (!block && query[cursor] === "\\") cursor += 2;
      else cursor += 1;
    }
    const inner = query.slice(contentStart, cursor);
    if (closed && isExactVariableReference(inner)) {
      output += query.slice(index, cursor + closing.length);
    } else {
      output += block ? '"""[REDACTED]"""' : '"[REDACTED]"';
      if (!closed) return output;
    }
    index = closed ? cursor + closing.length : query.length;
  }
  return output;
}

interface GraphqlStringLiteral {
  start: number;
  value: string;
}

/** Extract decoded-ish string literal values for response redaction. */
export function extractGraphqlStringLiterals(query: string): string[] {
  return scanGraphqlStringLiterals(query).map((literal) => literal.value);
}

/** Extract only literals assigned to credential-shaped GraphQL arguments. */
export function extractGraphqlCredentialLiterals(query: string): string[] {
  const values: string[] = [];
  let index = 0;
  while (index < query.length) {
    const current = query[index]!;
    if (current === " " || current === "\t" || current === "\r" || current === "\n" || current === ",") {
      index += 1;
      continue;
    }
    if (current === "#") {
      const newline = query.indexOf("\n", index);
      index = newline < 0 ? query.length : newline + 1;
      continue;
    }
    if (/[A-Za-z_]/.test(current)) {
      const start = index++;
      while (index < query.length && /[A-Za-z0-9_]/.test(query[index]!)) index += 1;
      const name = query.slice(start, index);
      const afterName = skipGraphqlIgnored(query, index);
      if (query[afterName] !== ":") continue;
      const valueStart = skipGraphqlIgnored(query, afterName + 1);
      if (query[valueStart] !== '"') continue;
      const literal = scanGraphqlLiteral(query, valueStart);
      if (isGraphqlCredentialName(name) && literal.value) values.push(literal.value);
      index = literal.next;
      continue;
    }
    if (current === '"') {
      index = scanGraphqlLiteral(query, index).next;
      continue;
    }
    index += 1;
  }
  return values;
}

function scanGraphqlStringLiterals(query: string): GraphqlStringLiteral[] {
  const values: GraphqlStringLiteral[] = [];
  let index = 0;
  while (index < query.length) {
    if (query[index] !== '"') {
      index += 1;
      continue;
    }
    const literal = scanGraphqlLiteral(query, index);
    if (literal.value) values.push({ start: index, value: literal.value });
    if (!literal.closed) break;
    index = literal.next;
  }
  return values;
}

function skipGraphqlIgnored(query: string, start: number): number {
  let index = start;
  while (index < query.length) {
    if (
      query[index] === " " ||
      query[index] === "\t" ||
      query[index] === "\r" ||
      query[index] === "\n" ||
      query[index] === ","
    ) {
      index += 1;
    } else if (query[index] === "#") {
      const newline = query.indexOf("\n", index);
      index = newline < 0 ? query.length : newline + 1;
    } else {
      break;
    }
  }
  return index;
}

function scanGraphqlLiteral(
  query: string,
  start: number,
): {
  next: number;
  closed: boolean;
  value: string;
} {
  const block = query.slice(start, start + 3) === '"""';
  const opening = block ? 3 : 1;
  const closing = block ? '"""' : '"';
  const contentStart = start + opening;
  let cursor = contentStart;
  while (cursor < query.length) {
    if (query.slice(cursor, cursor + closing.length) === closing) {
      const raw = query.slice(contentStart, cursor);
      if (!raw) return { next: cursor + closing.length, closed: true, value: "" };
      if (block) return { next: cursor + closing.length, closed: true, value: raw };
      try {
        return {
          next: cursor + closing.length,
          closed: true,
          value: JSON.parse(query.slice(start, cursor + closing.length)) as string,
        };
      } catch {
        return { next: cursor + closing.length, closed: true, value: raw };
      }
    }
    if (block && query.startsWith('\\"""', cursor)) cursor += 4;
    else if (!block && query[cursor] === "\\") cursor += 2;
    else cursor += 1;
  }
  return { next: query.length, closed: false, value: query.slice(contentStart) };
}
