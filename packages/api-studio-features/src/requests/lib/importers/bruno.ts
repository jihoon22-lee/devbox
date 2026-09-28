import { emptyMultipartPart, safeMultipartFileName } from "../multipart";
import { MAX_EXPORTED_COLLECTIONS } from "../transfer";
import { emptyRequest, IMPORT_METHODS, ImportError, type ImportBundle, normalizeBodyHeaders } from "./model";
export interface BruEntry {
  key: string;
  value: string;
  enabled: boolean;
}
export interface BruBlock {
  name: string;
  kind: "dict" | "text" | "list";
  entries?: BruEntry[];
  text?: string;
  items?: string[];
}
function dict(lines: string[]): BruEntry[] {
  return lines
    .filter((line) => line.trim() && !line.trimStart().startsWith("#"))
    .map((line) => {
      const at = line.indexOf(":");
      if (at < 1) throw new ImportError("가져올 수 없는 형식입니다.");
      const key = line.slice(0, at).trim();
      return { key: key.replace(/^~/, ""), value: line.slice(at + 1).trim(), enabled: !key.startsWith("~") };
    });
}
export function parseBruBlocks(text: string): BruBlock[] {
  if (new TextEncoder().encode(text).length > 16 * 1024 * 1024) throw new ImportError("가져올 수 없는 형식입니다.");
  const lines = text.replace(/^\uFEFF/, "").split(/\r?\n/),
    blocks: BruBlock[] = [];
  for (let i = 0; i < lines.length; i++) {
    if (!lines[i].trim() || lines[i].trimStart().startsWith("#")) continue;
    const match = /^([A-Za-z][A-Za-z0-9:_-]*)\s+([\[{])\s*$/.exec(lines[i]);
    if (!match) throw new ImportError("가져올 수 없는 형식입니다.");
    const name = match[1],
      close = match[2] === "[" ? "]" : "}",
      content: string[] = [];
    while (++i < lines.length && lines[i].trimEnd() !== close)
      content.push(lines[i].startsWith("  ") ? lines[i].slice(2) : lines[i]);
    if (i === lines.length) throw new ImportError("가져올 수 없는 형식입니다.");
    if (match[2] === "[")
      blocks.push({
        name,
        kind: "list",
        items: content.map((line) => line.trim()).filter((line) => line && !line.startsWith("#")),
      });
    else if (/^(body:|script:)/.test(name) || ["tests", "docs"].includes(name))
      blocks.push({ name, kind: "text", text: content.join("\n") });
    else blocks.push({ name, kind: "dict", entries: dict(content) });
  }
  return blocks;
}
export function parseBruno(files: { relativePath: string; text: string }[]): ImportBundle {
  if (
    files.length > 500 ||
    files.reduce((sum, file) => sum + new TextEncoder().encode(file.text).length, 0) > 32 * 1024 * 1024
  )
    throw new ImportError("가져올 수 없는 형식입니다.");
  const bundle: ImportBundle = { requests: [], environments: [], warnings: [] };
  let scripts = 0,
    truncated = false;
  for (const file of files) {
    const path = file.relativePath.replace(/\\/g, "/"),
      parts = path.split("/");
    if (path.startsWith("/") || parts.some((part) => !part || part === "." || part === ".." || part.includes(":")))
      throw new ImportError("가져올 수 없는 형식입니다.");
    const blocks = parseBruBlocks(file.text),
      find = (name: string) => blocks.find((block) => block.name === name);
    const entries = (name: string) => {
      const block = find(name);
      return block?.entries ?? (block?.text ? dict(block.text.split("\n")) : []);
    };
    const get = (block: string, key: string) =>
      entries(block).find((entry) => entry.key === key && entry.enabled)?.value ?? "";
    const body = (name: string) => find(name)?.text ?? "";
    scripts += blocks.filter((block) => block.name.startsWith("script:") || block.name === "tests").length;
    const methods = blocks.filter((block) => IMPORT_METHODS.some((method) => method.toLowerCase() === block.name));
    if (
      parts.includes("environments") ||
      (methods.length === 0 && blocks.some((block) => block.name === "vars" || block.name === "vars:secret"))
    ) {
      const vars = new Map(
        entries("vars")
          .filter((entry) => entry.enabled)
          .map((entry) => [
            entry.key,
            { key: entry.key, value: entry.value } as { key: string; value: string; secret?: boolean },
          ]),
      );
      for (const key of find("vars:secret")?.items ?? []) vars.set(key, { key, value: "", secret: true });
      bundle.environments.push({ name: parts[parts.length - 1].replace(/\.bru$/i, ""), variables: [...vars.values()] });
      continue;
    }
    if (methods.length === 0) {
      bundle.warnings.push("요청이 없는 Bruno 설정 파일은 가져오지 않았습니다.");
      continue;
    }
    if (methods.length !== 1) throw new ImportError("가져올 수 없는 형식입니다.");
    if (bundle.requests.length >= MAX_EXPORTED_COLLECTIONS) {
      truncated = true;
      continue;
    }
    const method = methods[0].name,
      request = emptyRequest(),
      name = get("meta", "name") || parts[parts.length - 1].replace(/\.bru$/i, "");
    request.method = method.toUpperCase();
    request.url = get(method, "url");
    if (!request.url) throw new ImportError("가져올 수 없는 형식입니다.");
    request.headers = entries("headers").map((entry) => ({ ...entry }));
    request.params = entries(find("params:query") ? "params:query" : "query")
      .filter((entry) => entry.enabled)
      .map(({ key, value }) => ({ key, value }));
    const kind = get(method, "body");
    if (kind === "json") {
      request.body_kind = "json";
      request.body = body("body:json");
    } else if (["text", "xml"].includes(kind)) {
      request.body_kind = "raw";
      request.body = body(`body:${kind}`);
    } else if (kind === "form-urlencoded") {
      request.body_kind = "form";
      request.body = entries("body:form-urlencoded")
        .filter((entry) => entry.enabled)
        .map((entry) => `${encodeURIComponent(entry.key)}=${encodeURIComponent(entry.value)}`)
        .join("\n");
    } else if (kind === "multipart-form") {
      request.body_kind = "multipart";
      request.multipart = entries("body:multipart-form").map((entry) => {
        if (entry.value.startsWith("@")) {
          bundle.warnings.push(`파일 파트 ${entry.key}는 파일을 다시 선택해야 합니다.`);
          const file = entry.value.replace(/^@file\((.*)\)$/, "$1").replace(/^@/, "");
          return {
            ...emptyMultipartPart("file"),
            name: entry.key,
            file_name: safeMultipartFileName(file.replace(/\\/g, "/").split("/").pop() ?? ""),
            enabled: entry.enabled,
          };
        }
        return { ...emptyMultipartPart(), name: entry.key, value: entry.value, enabled: entry.enabled };
      });
    } else if (kind === "graphql") {
      request.body_kind = "graphql";
      request.graphql = {
        query: body("body:graphql"),
        variables: body("body:graphql:vars") || "{}",
        operation_name: "",
      };
    } else if (kind && kind !== "none") bundle.warnings.push(`${name}: 지원하지 않는 본문 형식은 가져오지 않았습니다.`);
    const auth = get(method, "auth"),
      none = { kind: "none", username: "", password: "", token: "", api_key: "", api_value: "" };
    if (auth === "bearer") request.auth = { ...none, kind: auth, token: get("auth:bearer", "token") };
    else if (auth === "basic")
      request.auth = {
        ...none,
        kind: auth,
        username: get("auth:basic", "username"),
        password: get("auth:basic", "password"),
      };
    else if (auth === "apikey") {
      const key = get("auth:apikey", "key"),
        value = get("auth:apikey", "value");
      if (get("auth:apikey", "placement") === "query") request.params.push({ key, value });
      else request.auth = { ...none, kind: auth, api_key: key, api_value: value };
    } else if (auth && auth !== "none") bundle.warnings.push(`${name}: 지원하지 않는 인증 방식은 가져오지 않았습니다.`);
    normalizeBodyHeaders(request);
    bundle.requests.push({ name, folder: parts.slice(0, -1).join("/"), request });
  }
  if (scripts) bundle.warnings.push(`스크립트·테스트 블록 ${scripts}개는 가져오지 않았습니다.`);
  if (truncated) bundle.warnings.push(`요청이 많아 처음 ${MAX_EXPORTED_COLLECTIONS}개만 가져왔습니다.`);
  return bundle;
}
