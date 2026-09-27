import { emptyMultipartPart, safeMultipartFileName } from "../multipart";
import { emptyRequest, IMPORT_METHODS, ImportError, type ImportBundle, normalizeBodyHeaders } from "./model";
import { formRows, object, optionalObject, readJson, rows, text } from "./parsing";
const browserHeaders = new Set(["host", "content-length", "connection", "accept-encoding", "cookie"]);
export function parseHar(source: string): ImportBundle {
  const log = object(readJson(source).log);
  if (!Array.isArray(log.entries)) throw new ImportError("가져올 수 없는 형식입니다.");
  const bundle: ImportBundle = { requests: [], environments: [], warnings: [] };
  const seen = new Set<string>();
  let duplicates = 0,
    nonHttp = 0,
    unsupported = 0;
  for (const entry of log.entries) {
    if (bundle.requests.length === 500) {
      bundle.warnings.push("요청이 많아 처음 500개만 가져왔습니다.");
      break;
    }
    const input = object(object(entry).request),
      request = emptyRequest();
    let url: URL;
    try {
      url = new URL(text(input.url));
    } catch {
      throw new ImportError("가져올 수 없는 형식입니다.");
    }
    if (!["http:", "https:"].includes(url.protocol)) {
      nonHttp++;
      continue;
    }
    request.method = text(input.method, "GET").toUpperCase();
    if (!IMPORT_METHODS.some((method) => method === request.method)) {
      unsupported++;
      continue;
    }
    request.url = text(input.url);
    const headers = rows(input.headers).map(object);
    request.headers = headers
      .filter((row) => !text(row.name).startsWith(":") && !browserHeaders.has(text(row.name).toLowerCase()))
      .map((row) => ({ key: text(row.name), value: text(row.value), enabled: true }));
    const cookies = rows(input.cookies).map(object);
    if (cookies.length)
      request.cookies = cookies.map((cookie) => ({
        name: text(cookie.name),
        value: text(cookie.value),
        enabled: true,
      }));
    else
      for (const header of headers.filter((row) => text(row.name).toLowerCase() === "cookie"))
        for (const cookie of text(header.value).split(";")) {
          const at = cookie.indexOf("=");
          if (at > 0)
            request.cookies.push({
              name: cookie.slice(0, at).trim(),
              value: cookie.slice(at + 1).trim(),
              enabled: true,
            });
        }
    const body = optionalObject(input.postData),
      mime = text(body.mimeType).toLowerCase().split(";")[0];
    if (mime.includes("json")) {
      request.body_kind = "json";
      request.body = text(body.text);
    } else if (mime === "application/x-www-form-urlencoded") {
      request.body_kind = "form";
      const params = rows(body.params).map(object);
      request.body = params.length
        ? formRows(params.map((row) => ({ key: text(row.name), value: text(row.value) })))
        : text(body.text).replace(/&/g, "\n");
    } else if (mime === "multipart/form-data") {
      request.body_kind = "multipart";
      request.multipart = rows(body.params)
        .map(object)
        .map((row) => {
          const name = text(row.name);
          if (row.fileName) {
            bundle.warnings.push(`파일 파트 ${name}는 파일을 다시 선택해야 합니다.`);
            return {
              ...emptyMultipartPart("file"),
              name,
              file_name: safeMultipartFileName(text(row.fileName).replace(/\\/g, "/").split("/").pop() ?? ""),
              content_type: text(row.contentType),
            };
          }
          return { ...emptyMultipartPart(), name, value: text(row.value), content_type: text(row.contentType) };
        });
    } else if (body.text !== undefined) {
      request.body_kind = "raw";
      request.body = text(body.text);
    }
    normalizeBodyHeaders(request);
    const key = JSON.stringify([request.method, request.url, request.body_kind, request.body, request.multipart]);
    if (seen.has(key)) {
      duplicates++;
      continue;
    }
    seen.add(key);
    bundle.requests.push({ name: `${request.method} ${url.pathname || "/"}`, folder: url.host, request });
  }
  if (duplicates || nonHttp)
    bundle.warnings.push(`같은 요청 ${duplicates}개와 http(s)가 아닌 요청 ${nonHttp}개를 건너뛰었습니다.`);
  if (unsupported) bundle.warnings.push(`지원하지 않는 요청 방법 ${unsupported}개를 건너뛰었습니다.`);
  return bundle;
}
