export class JsonPathError extends Error {}
type Token = { kind: "member" | "descend"; name: string } | { kind: "index"; index: number } | { kind: "wildcard" };
const invalid = () => new JsonPathError("지원하지 않는 JSONPath입니다");
function tokens(path: string): Token[] {
  if (path.length > 256 || !path.startsWith("$")) throw invalid();
  const result: Token[] = [];
  let offset = 1;
  while (offset < path.length) {
    if (path[offset] === ".") {
      offset++;
      const descend = path[offset] === ".";
      if (descend) offset++;
      if (path[offset] === "*") {
        result.push(descend ? { kind: "descend", name: "*" } : { kind: "wildcard" });
        offset++;
        continue;
      }
      const member = /^[A-Za-z_$][A-Za-z0-9_$-]*/.exec(path.slice(offset));
      if (!member) throw invalid();
      result.push({ kind: descend ? "descend" : "member", name: member[0] });
      offset += member[0].length;
    } else if (path[offset] === "[") {
      offset++;
      const quote = path[offset];
      if (quote === "'" || quote === '"') {
        offset++;
        let name = "";
        let closed = false;
        while (offset < path.length) {
          const character = path[offset++];
          if (character === quote) {
            closed = true;
            break;
          }
          if (character === "\\") {
            const escaped = path[offset++];
            if (escaped === quote || escaped === "\\" || escaped === "/") name += escaped;
            else if (escaped === "u" && /^[0-9a-fA-F]{4}$/.test(path.slice(offset, offset + 4))) {
              name += String.fromCharCode(Number.parseInt(path.slice(offset, offset + 4), 16));
              offset += 4;
            } else if (escaped && "bfnrt".includes(escaped))
              name += ({ b: "\b", f: "\f", n: "\n", r: "\r", t: "\t" } as Record<string, string>)[escaped];
            else throw invalid();
          } else {
            if (character.charCodeAt(0) < 32) throw invalid();
            name += character;
          }
        }
        if (!closed || path[offset++] !== "]") throw invalid();
        result.push({ kind: "member", name });
      } else {
        const bracket = /^(\*|-?\d+)\]/.exec(path.slice(offset));
        if (!bracket) throw invalid();
        offset += bracket[0].length;
        if (bracket[1] === "*") result.push({ kind: "wildcard" });
        else {
          const index = Number(bracket[1]);
          if (!Number.isSafeInteger(index)) throw invalid();
          result.push({ kind: "index", index });
        }
      }
    } else throw invalid();
  }
  return result;
}
export function evaluateJsonPath(value: unknown, path: string): unknown[] {
  const steps = tokens(path);
  let visited = 0;
  const visit = () => {
    if (++visited > 10000) throw new JsonPathError("JSONPath 탐색 한도를 넘었습니다");
  };
  const object = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === "object";
  const children = (value: unknown): unknown[] => {
    if (!object(value)) return [];
    const result: unknown[] = [];
    for (const key of Object.keys(value)) {
      visit();
      result.push(value[key]);
    }
    return result;
  };
  let current = [value];
  for (const token of steps) {
    const next: unknown[] = [];
    for (const node of current) {
      visit();
      if (token.kind === "member") {
        if (object(node) && Object.prototype.hasOwnProperty.call(node, token.name)) next.push(node[token.name]);
      } else if (token.kind === "index") {
        if (Array.isArray(node)) {
          const index = token.index < 0 ? node.length + token.index : token.index;
          if (index >= 0 && index < node.length) next.push(node[index]);
        }
      } else if (token.kind === "wildcard") next.push(...children(node));
      else {
        const pending = [node];
        while (pending.length) {
          const item = pending.pop();
          visit();
          const nested = children(item);
          if (token.name === "*") next.push(...nested);
          else if (object(item) && Object.prototype.hasOwnProperty.call(item, token.name)) next.push(item[token.name]);
          pending.push(...nested.reverse());
        }
      }
    }
    current = next;
  }
  return current;
}
