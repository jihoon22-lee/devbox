import { emptyMultipartPart, safeMultipartFileName } from "../multipart";
import {
  emptyRequest,
  IMPORT_METHODS,
  ImportError,
  type ImportBundle,
  normalizeBodyHeaders,
  requestName,
} from "./model";

// This is a tokenizer only: substitutions, commands and files are never evaluated.
export function tokenizeShell(command: string): string[] {
  if (command.length > 16 * 1024 * 1024) throw new ImportError("가져올 수 없는 형식입니다.");
  let text = command.replace(/[\\^`]\r?\n/g, " ");
  if (text.includes('^"')) text = text.replace(/\^(.)/g, "$1");
  const tokens: string[] = [];
  let token = "",
    active = false,
    quote: "single" | "double" | "ansi" | null = null;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quote === null) {
      if (/\s/.test(c)) {
        if (active) {
          tokens.push(token);
          token = "";
          active = false;
        }
      } else if (c === "'" || c === '"') {
        quote = c === "'" ? "single" : "double";
        active = true;
      } else if (c === "$" && text[i + 1] === "'") {
        quote = "ansi";
        active = true;
        i++;
      } else if (c === "\\") {
        if (i + 1 >= text.length) throw new ImportError("가져올 수 없는 형식입니다.");
        token += text[++i];
        active = true;
      } else {
        token += c;
        active = true;
      }
    } else if ((quote === "double" && c === '"') || (quote !== "double" && c === "'")) quote = null;
    else if (c === "\\" && quote !== "single") {
      const next = text[i + 1];
      if (next === undefined) throw new ImportError("가져올 수 없는 형식입니다.");
      if (quote === "ansi") {
        token +=
          ({ n: "\n", r: "\r", t: "\t", "\\": "\\", "'": "'", '"': '"' } as Record<string, string>)[next] ??
          `\\${next}`;
        i++;
      } else if (['"', "\\", "$", "`"].includes(next)) {
        token += next;
        i++;
      } else token += c;
    } else token += c;
    if (tokens.length > 10000) throw new ImportError("가져올 수 없는 형식입니다.");
  }
  if (quote) throw new ImportError("가져올 수 없는 형식입니다.");
  if (active) tokens.push(token);
  return tokens;
}
const valued = new Set([
  "-X",
  "--request",
  "-H",
  "--header",
  "-d",
  "--data",
  "--data-raw",
  "--data-ascii",
  "--data-binary",
  "--data-urlencode",
  "--json",
  "-F",
  "--form",
  "-u",
  "--user",
  "-b",
  "--cookie",
  "-A",
  "--user-agent",
  "-e",
  "--referer",
  "--url",
  "--max-time",
  "-o",
  "--output",
]);
const ignoredValues = new Set([
  "--retry",
  "--retry-delay",
  "--retry-max-time",
  "--connect-timeout",
  "-w",
  "--write-out",
  "-x",
  "--proxy",
  "--resolve",
  "--cacert",
  "-E",
  "--cert",
  "--key",
  "-c",
  "--cookie-jar",
  "-D",
  "--dump-header",
  "-T",
  "--upload-file",
  "--limit-rate",
  "--interface",
]);
const ignoredFlags = new Set(["-L", "-s", "-S", "-v", "-i", "--compressed"]);
const basename = (value: string) => safeMultipartFileName(value.replace(/\\/g, "/").split("/").pop() ?? "");
export function parseCurl(command: string): ImportBundle {
  const tokens = tokenizeShell(command);
  if (!["curl", "curl.exe"].includes(tokens.shift() ?? "")) throw new ImportError("curl 명령이 아닙니다");
  const request = emptyRequest(),
    warnings: string[] = [],
    data: string[] = [];
  let explicitMethod: string | undefined,
    get = false,
    json = false,
    hasData = false,
    contentType = "";
  const header = (key: string, value: string) => request.headers.push({ key, value, enabled: true });
  const fileWarning = (file: string) => warnings.push(`파일 본문(@${basename(file)})은 가져오지 않았습니다.`);
  for (let i = 0; i < tokens.length; i++) {
    let option = tokens[i],
      attached: string | undefined;
    if (option.startsWith("--") && option.includes("=")) {
      const at = option.indexOf("=");
      attached = option.slice(at + 1);
      option = option.slice(0, at);
    } else if (/^-[^-].+/.test(option)) {
      if (valued.has(option.slice(0, 2)) || ignoredValues.has(option.slice(0, 2))) {
        attached = option.slice(2);
        option = option.slice(0, 2);
      } else {
        const flags = option
          .slice(1)
          .split("")
          .map((c) => `-${c}`);
        if (flags.every((flag) => ignoredFlags.has(flag) || ["-G", "-I", "-k"].includes(flag))) {
          tokens.splice(i, 1, ...flags);
          option = tokens[i];
        }
      }
    }
    let value = "";
    if (valued.has(option) || ignoredValues.has(option)) {
      value = attached ?? tokens[++i];
      if (value === undefined) throw new ImportError("가져올 수 없는 형식입니다.");
    }
    if (ignoredValues.has(option)) {
      warnings.push(`지원하지 않는 옵션 ${option}를 건너뛰었습니다.`);
      continue;
    }
    if (ignoredFlags.has(option)) continue;
    switch (option) {
      case "-X":
      case "--request":
        explicitMethod = value.toUpperCase();
        break;
      case "-G":
      case "--get":
        get = true;
        break;
      case "-I":
      case "--head":
        explicitMethod = "HEAD";
        break;
      case "-k":
      case "--insecure":
        request.tls = { credentialId: null, verify: false };
        break;
      case "--url":
        request.url = value;
        break;
      case "-o":
      case "--output":
        break;
      case "--max-time": {
        const ms = Number(value) * 1000;
        if (!Number.isFinite(ms) || ms <= 0 || ms > 300000) throw new ImportError("가져올 수 없는 형식입니다.");
        request.timeout_ms = Math.round(ms);
        break;
      }
      case "-H":
      case "--header": {
        const at = value.indexOf(":");
        if (at < 1) throw new ImportError("가져올 수 없는 형식입니다.");
        const key = value.slice(0, at).trim(),
          content = value.slice(at + 1).trim();
        if (key.toLowerCase() === "authorization" && /^Bearer\s+/i.test(content))
          request.auth = { ...request.auth!, kind: "bearer", token: content.replace(/^Bearer\s+/i, "") };
        else header(key, content);
        if (key.toLowerCase() === "content-type") contentType = content.toLowerCase();
        break;
      }
      case "-u":
      case "--user": {
        const at = value.indexOf(":");
        request.auth = {
          ...request.auth!,
          kind: "basic",
          username: at < 0 ? value : value.slice(0, at),
          password: at < 0 ? "" : value.slice(at + 1),
        };
        break;
      }
      case "-b":
      case "--cookie": {
        if (!value.includes("=")) {
          fileWarning(value.replace(/^@/, ""));
          break;
        }
        for (const cookie of value.split(";")) {
          const at = cookie.indexOf("=");
          if (at > 0)
            request.cookies.push({
              name: cookie.slice(0, at).trim(),
              value: cookie.slice(at + 1).trim(),
              enabled: true,
            });
        }
        break;
      }
      case "-A":
      case "--user-agent":
        header("User-Agent", value);
        break;
      case "-e":
      case "--referer":
        header("Referer", value);
        break;
      case "-F":
      case "--form": {
        const at = value.indexOf("=");
        if (at < 1) throw new ImportError("가져올 수 없는 형식입니다.");
        const name = value.slice(0, at),
          content = value.slice(at + 1);
        if (/^[@<]/.test(content)) {
          request.multipart.push({
            ...emptyMultipartPart("file"),
            name,
            file_name: basename(content.slice(1).split(";")[0]),
          });
          warnings.push(`파일 파트 ${name}는 파일을 다시 선택해야 합니다.`);
        } else request.multipart.push({ ...emptyMultipartPart(), name, value: content });
        break;
      }
      case "--data-urlencode": {
        hasData = true;
        const at = value.indexOf("=");
        if (at < 0 && value.includes("@")) {
          fileWarning(value.slice(value.indexOf("@") + 1));
          break;
        }
        data.push(
          at < 0 ? encodeURIComponent(value) : `${value.slice(0, at)}=${encodeURIComponent(value.slice(at + 1))}`,
        );
        break;
      }
      case "--json":
        json = true;
        hasData = true;
        if (value.startsWith("@")) fileWarning(value.slice(1));
        else data.push(value);
        break;
      case "-d":
      case "--data":
      case "--data-raw":
      case "--data-ascii":
      case "--data-binary":
        hasData = true;
        if (value.startsWith("@") && option !== "--data-raw") fileWarning(value.slice(1));
        else data.push(value);
        break;
      default:
        if (option.startsWith("-")) warnings.push(`지원하지 않는 옵션 ${option}를 건너뛰었습니다.`);
        else if (!request.url) request.url = option;
        else warnings.push("추가 URL은 가져오지 않았습니다.");
    }
  }
  if (!request.url) throw new ImportError("가져올 수 없는 형식입니다.");
  if (!/^[a-z][a-z\d+.-]*:\/\//i.test(request.url) && !request.url.startsWith("{{"))
    request.url = `http://${request.url}`;
  if (/^[a-z][a-z\d+.-]*:\/\//i.test(request.url) && !/^https?:\/\//i.test(request.url))
    throw new ImportError("가져올 수 없는 형식입니다.");
  request.method = explicitMethod ?? (get ? "GET" : hasData || request.multipart.length ? "POST" : "GET");
  if (!IMPORT_METHODS.some((method) => method === request.method)) throw new ImportError("가져올 수 없는 형식입니다.");
  if (get && data.length) {
    const [base, ...fragment] = request.url.split("#");
    request.url =
      base + (base.includes("?") ? "&" : "?") + data.join("&") + (fragment.length ? `#${fragment.join("#")}` : "");
  } else if (request.multipart.length) request.body_kind = "multipart";
  else if (data.length) {
    request.body_kind =
      json || contentType.includes("json")
        ? "json"
        : !contentType || contentType.includes("application/x-www-form-urlencoded")
          ? "form"
          : "raw";
    request.body = data.join("&");
    if (request.body_kind === "form") request.body = request.body.replace(/&/g, "\n");
  }
  if (json && !request.headers.some((h) => h.key.toLowerCase() === "accept")) header("Accept", "application/json");
  normalizeBodyHeaders(request);
  return { requests: [{ name: requestName(request), folder: "", request }], environments: [], warnings };
}
