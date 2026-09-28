import type { AuthConfig, RequestTemplate } from "../../types";
import { emptyMultipartPart, safeMultipartFileName } from "../multipart";
import { MAX_EXPORTED_COLLECTIONS } from "../transfer";
import { emptyRequest, IMPORT_METHODS, ImportError, type ImportBundle, normalizeBodyHeaders } from "./model";
import { formRows, object, optionalObject, readJson, rows, text, type JsonObject } from "./parsing";
const blankAuth = (): AuthConfig => ({
  kind: "none",
  username: "",
  password: "",
  token: "",
  api_key: "",
  api_value: "",
});
function applyAuth(request: RequestTemplate, input: unknown, name: string, warnings: string[]) {
  const config = optionalObject(input),
    kind = text(config.type, "noauth");
  const values = rows(config[kind]).map(object);
  const get = (key: string) => text(values.find((item) => item.key === key)?.value);
  request.auth = blankAuth();
  if (kind === "noauth") return;
  if (kind === "bearer") request.auth = { ...blankAuth(), kind, token: get("token") };
  else if (kind === "basic")
    request.auth = { ...blankAuth(), kind, username: get("username"), password: get("password") };
  else if (kind === "apikey") {
    if (get("in") === "query") {
      request.params.push({ key: get("key"), value: get("value") });
      warnings.push(`${name}: API 키 인증을 쿼리 파라미터로 옮겼습니다.`);
    } else request.auth = { ...blankAuth(), kind, api_key: get("key"), api_value: get("value") };
  } else warnings.push(`${name}: 지원하지 않는 인증 방식(${kind})은 가져오지 않았습니다.`);
}
function applyBody(request: RequestTemplate, input: unknown, name: string, warnings: string[]) {
  const body = optionalObject(input);
  switch (body.mode) {
    case undefined:
      break;
    case "raw": {
      const options = optionalObject(body.options),
        raw = optionalObject(options.raw);
      request.body_kind =
        raw.language === "json" ||
        request.headers.some(
          (h) =>
            h.enabled !== false && h.key.toLowerCase() === "content-type" && h.value.toLowerCase().includes("json"),
        )
          ? "json"
          : "raw";
      request.body = text(body.raw);
      break;
    }
    case "urlencoded":
      request.body_kind = "form";
      request.body = formRows(body.urlencoded);
      break;
    case "formdata":
      request.body_kind = "multipart";
      request.multipart = rows(body.formdata)
        .map(object)
        .map((part) => {
          const key = text(part.key),
            enabled = part.disabled !== true;
          if (part.type === "file") {
            warnings.push(`${name}: 파일 파트 ${key}는 파일을 다시 선택해야 합니다.`);
            const source = Array.isArray(part.src) ? text(part.src[0]) : text(part.src);
            return {
              ...emptyMultipartPart("file"),
              name: key,
              file_name: safeMultipartFileName(source.replace(/\\/g, "/").split("/").pop() ?? ""),
              content_type: text(part.contentType),
              enabled,
            };
          }
          return {
            ...emptyMultipartPart(),
            name: key,
            value: text(part.value),
            content_type: text(part.contentType),
            enabled,
          };
        });
      break;
    case "graphql": {
      const graph = object(body.graphql);
      request.body_kind = "graphql";
      request.graphql = {
        query: text(graph.query),
        variables: typeof graph.variables === "string" ? graph.variables : JSON.stringify(graph.variables ?? {}),
        operation_name: "",
      };
      break;
    }
    case "file":
      warnings.push(`${name}: 파일 본문은 가져오지 않았습니다.`);
      break;
    default:
      warnings.push(`${name}: 지원하지 않는 본문 형식은 가져오지 않았습니다.`);
  }
  normalizeBodyHeaders(request);
}
export function parsePostman(source: string): ImportBundle {
  const root = readJson(source),
    info = object(root.info);
  if (!/collection\/v2\.[01](?:\.|\/)/.test(text(info.schema))) throw new ImportError("가져올 수 없는 형식입니다.");
  const bundle: ImportBundle = { requests: [], environments: [], warnings: [] };
  let scripts = rows(root.event).length,
    visited = 0,
    truncated = false;
  const walk = (items: unknown, folder: string, inherited: unknown, depth: number) => {
    if (depth > 8) throw new ImportError("가져올 수 없는 형식입니다.");
    for (const value of rows(items)) {
      if (++visited > 10000) throw new ImportError("가져올 수 없는 형식입니다.");
      const item = object(value),
        name = text(item.name, "untitled");
      scripts += rows(item.event).length;
      const ownAuth = item.auth ?? inherited;
      if (item.item !== undefined) {
        walk(item.item, folder ? `${folder}/${name}` : name, ownAuth, depth + 1);
        continue;
      }
      if (bundle.requests.length >= MAX_EXPORTED_COLLECTIONS) {
        truncated = true;
        continue;
      }
      const input: JsonObject =
        typeof item.request === "string" ? { url: item.request, method: "GET" } : object(item.request);
      const request = emptyRequest();
      request.method = text(input.method, "GET").toUpperCase();
      request.url = typeof input.url === "string" ? input.url : text(optionalObject(input.url).raw);
      if (!request.url || !IMPORT_METHODS.some((method) => method === request.method))
        throw new ImportError("가져올 수 없는 형식입니다.");
      request.headers = rows(input.header)
        .map(object)
        .map((row) => ({ key: text(row.key), value: text(row.value), enabled: row.disabled !== true }));
      applyAuth(request, input.auth ?? ownAuth, name, bundle.warnings);
      applyBody(request, input.body, name, bundle.warnings);
      bundle.requests.push({ name, folder, request });
    }
  };
  walk(root.item, "", root.auth, 0);
  if (scripts) bundle.warnings.push(`스크립트 ${scripts}개는 가져오지 않았습니다.`);
  if (truncated) bundle.warnings.push(`요청이 많아 처음 ${MAX_EXPORTED_COLLECTIONS}개만 가져왔습니다.`);
  const variables = rows(root.variable)
    .map(object)
    .filter((row) => row.disabled !== true)
    .map((row) => ({
      key: text(row.key),
      value: typeof row.value === "string" ? row.value : JSON.stringify(row.value ?? ""),
    }));
  if (variables.length) bundle.environments.push({ name: `Postman: ${text(info.name, "untitled")}`, variables });
  return bundle;
}
