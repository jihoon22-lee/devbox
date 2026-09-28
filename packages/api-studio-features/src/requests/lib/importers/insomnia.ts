import { emptyMultipartPart, safeMultipartFileName } from "../multipart";
import { MAX_EXPORTED_COLLECTIONS, MAX_EXPORTED_ENVIRONMENTS } from "../transfer";
import { emptyRequest, IMPORT_METHODS, ImportError, type ImportBundle } from "./model";
import { normalizeBodyHeaders } from "./model";
import { formRows, object, optionalObject, readJson, rows, text, type JsonObject } from "./parsing";
const normalize = (value: string) => value.replace(/\{\{\s*(?:_\.\s*)?([A-Za-z_][A-Za-z0-9_.-]*)\s*\}\}/g, "{{$1}}");
export function parseInsomnia(source: string): ImportBundle {
  const document = readJson(source);
  if (document._type !== "export" || document.__export_format !== 4)
    throw new ImportError("가져올 수 없는 형식입니다.");
  const resources = rows(document.resources).map(object);
  if (resources.length > 10000) throw new ImportError("가져올 수 없는 형식입니다.");
  const byId = new Map<string, JsonObject>();
  for (const resource of resources) {
    const id = text(resource._id);
    if (!id || byId.has(id)) throw new ImportError("가져올 수 없는 형식입니다.");
    byId.set(id, resource);
  }
  const bundle: ImportBundle = { requests: [], environments: [], warnings: [] };
  const ancestors = (resource: JsonObject): JsonObject[] => {
    const result: JsonObject[] = [],
      seen = new Set<string>([text(resource._id)]);
    let id = text(resource.parentId);
    while (id) {
      if (seen.has(id) || result.length >= 8) throw new ImportError("가져올 수 없는 형식입니다.");
      seen.add(id);
      const parent = byId.get(id);
      if (!parent) throw new ImportError("가져올 수 없는 형식입니다.");
      result.push(parent);
      id = text(parent.parentId);
    }
    return result;
  };
  const requests = resources.filter((resource) => resource._type === "request");
  if (requests.length > MAX_EXPORTED_COLLECTIONS)
    bundle.warnings.push(`요청이 많아 처음 ${MAX_EXPORTED_COLLECTIONS}개만 가져왔습니다.`);
  for (const input of requests.slice(0, MAX_EXPORTED_COLLECTIONS)) {
    const name = text(input.name, "untitled"),
      request = emptyRequest();
    const norm = (value: unknown) => normalize(text(value));
    request.method = text(input.method, "GET").toUpperCase();
    request.url = norm(input.url);
    if (!request.url || !IMPORT_METHODS.some((method) => method === request.method))
      throw new ImportError("가져올 수 없는 형식입니다.");
    request.headers = rows(input.headers)
      .map(object)
      .map((row) => ({ key: text(row.name), value: norm(row.value), enabled: row.disabled !== true }));
    request.params = rows(input.parameters)
      .map(object)
      .filter((row) => row.disabled !== true)
      .map((row) => ({ key: text(row.name), value: norm(row.value) }));
    const body = optionalObject(input.body),
      mime = text(body.mimeType).toLowerCase();
    if (body.fileName) {
      bundle.warnings.push(`${name}: 파일 본문은 가져오지 않았습니다.`);
    } else if (mime.includes("json")) {
      request.body_kind = "json";
      request.body = norm(body.text);
    } else if (mime === "application/x-www-form-urlencoded") {
      request.body_kind = "form";
      request.body = formRows(
        rows(body.params)
          .map(object)
          .map((row) => ({ ...row, key: text(row.name), value: norm(row.value) })),
      );
    } else if (mime === "multipart/form-data") {
      request.body_kind = "multipart";
      request.multipart = rows(body.params)
        .map(object)
        .map((row) => {
          const partName = text(row.name),
            enabled = row.disabled !== true;
          if (row.type === "file" || row.fileName) {
            bundle.warnings.push(`${name}: 파일 파트 ${partName}는 파일을 다시 선택해야 합니다.`);
            return {
              ...emptyMultipartPart("file"),
              name: partName,
              file_name: safeMultipartFileName(text(row.fileName).replace(/\\/g, "/").split("/").pop() ?? ""),
              enabled,
            };
          }
          return { ...emptyMultipartPart(), name: partName, value: norm(row.value), enabled };
        });
    } else if (mime === "application/graphql") {
      const graph = readJson(text(body.text));
      request.body_kind = "graphql";
      request.graphql = {
        query: norm(graph.query),
        variables: normalize(
          typeof graph.variables === "string" ? graph.variables : JSON.stringify(graph.variables ?? {}),
        ),
        operation_name: text(graph.operationName),
      };
    } else if (body.text) {
      request.body_kind = "raw";
      request.body = norm(body.text);
    }
    const auth = optionalObject(input.authentication),
      empty = { kind: "none", username: "", password: "", token: "", api_key: "", api_value: "" };
    if (auth.disabled !== true) {
      if (auth.type === "bearer") request.auth = { ...empty, kind: "bearer", token: norm(auth.token) };
      else if (auth.type === "basic")
        request.auth = { ...empty, kind: "basic", username: norm(auth.username), password: norm(auth.password) };
      else if (auth.type === "apikey" && auth.addTo === "header")
        request.auth = { ...empty, kind: "apikey", api_key: text(auth.key), api_value: norm(auth.value) };
      else if (auth.type && auth.type !== "none")
        bundle.warnings.push(`${name}: 지원하지 않는 인증 방식은 가져오지 않았습니다.`);
    }
    normalizeBodyHeaders(request);
    if (JSON.stringify(request).includes("{%"))
      bundle.warnings.push(`${name}: 템플릿 태그는 실행하지 않습니다. 직접 값을 확인해 주세요.`);
    const folder = ancestors(input)
      .filter((parent) => parent._type === "request_group")
      .reverse()
      .map((parent) => text(parent.name, "untitled"))
      .join("/");
    bundle.requests.push({ name, folder, request });
  }
  const environments = resources.filter((resource) => resource._type === "environment");
  if (environments.length > MAX_EXPORTED_ENVIRONMENTS)
    bundle.warnings.push(`환경이 많아 처음 ${MAX_EXPORTED_ENVIRONMENTS}개만 가져왔습니다.`);
  for (const environment of environments.slice(0, MAX_EXPORTED_ENVIRONMENTS)) {
    const values = new Map<string, unknown>(Object.entries(optionalObject(environment.data)));
    for (const ancestor of ancestors(environment).filter((parent) => parent._type === "environment"))
      for (const [key, value] of Object.entries(optionalObject(ancestor.data)))
        if (!values.has(key)) values.set(key, value);
    bundle.environments.push({
      name: text(environment.name, "Base Environment"),
      variables: [...values].map(([key, value]) => ({
        key,
        value: normalize(typeof value === "string" ? value : JSON.stringify(value ?? "")),
      })),
    });
  }
  const plugins = resources.filter((resource) => resource._type === "plugin").length;
  if (plugins) bundle.warnings.push(`플러그인 ${plugins}개는 가져오지 않았습니다.`);
  return bundle;
}
