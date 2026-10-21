import { type Plan, quote } from "./plan";

export function fetchCode(plan: Plan): string {
  const lines: string[] = [];
  if (!plan.verify || plan.credential)
    lines.push(
      'throw new Error("브라우저 fetch는 사용자 TLS 설정을 지원하지 않습니다. 다른 코드 대상을 사용하세요.");',
    );
  if (plan.parts.some((part) => !part.file && part.contentType))
    lines.push('throw new Error("브라우저 FormData는 텍스트 part Content-Type을 보존하지 않습니다.");');
  lines.push("const headers = new Headers([");
  for (const [key, value] of plan.headers) lines.push(`  [${quote(key)}, ${quote(value)}],`);
  lines.push("]); ");
  if (plan.multipart) {
    lines.push("const body = new FormData();");
    if (plan.parts.some((part) => part.file))
      lines.push("const files = new Map(); // 각 file 자리표시자 키에 직접 선택한 File 객체를 넣으세요.");
    for (const part of plan.parts)
      lines.push(
        part.file
          ? `body.append(${quote(part.name)}, files.get(${quote(part.value)}), ${quote(part.filename)}); // ${part.value}`
          : `body.append(${quote(part.name)}, ${quote(part.value)});${part.contentType ? ` // Content-Type: ${part.contentType}` : ""}`,
      );
  }
  lines.push(
    `const response = await fetch(${quote(plan.url)}, {`,
    `  method: ${quote(plan.method)},`,
    "  headers,",
    ...(plan.multipart ? ["  body,"] : plan.body ? [`  body: ${quote(plan.body)},`] : []),
    "});",
    "console.log(await response.text());",
  );
  return lines.join("\n");
}
/** Node.js axios. 헤더는 객체라 중복 헤더를 보존하지 못하면 실행 전에 멈춘다. */
export function axios(plan: Plan): string {
  const tls = !plan.verify || plan.credential;
  const lines = ['import axios from "axios";'];
  if (tls) lines.push('import https from "node:https";');
  if (plan.credential) lines.push('import { readFileSync } from "node:fs";');
  if (plan.parts.some((part) => part.file)) lines.push('import { openAsBlob } from "node:fs";');
  lines.push("");
  if (new Set(plan.headers.map(([key]) => key.toLowerCase())).size !== plan.headers.length)
    lines.push('throw new Error("axios는 중복 헤더를 보존하지 않습니다. curl·Go·Rust 대상을 사용하세요.");');
  if (plan.parts.some((part) => !part.file && part.contentType))
    lines.push('throw new Error("FormData는 텍스트 part Content-Type을 보존하지 않습니다.");');
  // 객체 리터럴의 "__proto__" 키는 프로토타입을 바꾸므로 Object.fromEntries로 자기 속성을 만든다.
  lines.push("const headers = Object.fromEntries([");
  for (const [key, value] of plan.headers) lines.push(`  [${quote(key)}, ${quote(value)}],`);
  lines.push("]);");
  if (plan.multipart) {
    lines.push("const body = new FormData();");
    for (const part of plan.parts)
      lines.push(
        part.file
          ? `body.append(${quote(part.name)}, await openAsBlob(${quote(part.value)}${part.contentType ? `, { type: ${quote(part.contentType)} }` : ""}), ${quote(part.filename)});`
          : `body.append(${quote(part.name)}, ${quote(part.value)});${part.contentType ? ` // Content-Type: ${part.contentType}` : ""}`,
      );
  }
  if (tls) {
    lines.push("const httpsAgent = new https.Agent({");
    if (!plan.verify) lines.push("  rejectUnauthorized: false,");
    if (plan.credential) {
      if (plan.verify) lines.push('  ca: readFileSync("{{ca_pem}}"),');
      lines.push('  cert: readFileSync("{{client_cert_pem}}"),', '  key: readFileSync("{{client_key_pem}}"),');
    }
    lines.push("});");
  }
  const timeout = Number.isFinite(plan.timeout) && plan.timeout > 0 ? Math.round(plan.timeout * 1000) : 30000;
  lines.push(
    "const response = await axios.request({",
    `  method: ${quote(plan.method)},`,
    `  url: ${quote(plan.url)},`,
    "  headers,",
    ...(plan.multipart ? ["  data: body,"] : plan.body ? [`  data: ${quote(plan.body)},`] : []),
    `  timeout: ${timeout},`,
    ...(tls ? ["  httpsAgent,"] : []),
    ...(plan.credential ? ["  maxRedirects: 0,"] : []),
    '  responseType: "text",',
    "  transformResponse: [(data) => data],",
    "  validateStatus: () => true,",
    "});",
    "console.log(response.data);",
  );
  return lines.join("\n");
}
