import type { RequestTemplate } from "../types";
import type { EnvVariable } from "./environments";
import { sanitizeRequestForPersistence } from "./persistence";
import { buildGraphqlBody } from "./graphql";
export type CodeTarget = "curl" | "fetch" | "python" | "go" | "csharp";
type Part = { name: string; value: string; file: boolean; filename: string; contentType: string };
type Plan = {
  method: string;
  url: string;
  headers: [string, string][];
  body: string;
  multipart: boolean;
  parts: Part[];
  verify: boolean;
  credential: boolean;
  timeout: number;
};
const quote = (value: string) =>
  JSON.stringify(value.replace(/[\uD800-\uDFFF]/gu, "�"))
    .replace(/\u2028/gu, "\\u2028")
    .replace(/\u2029/gu, "\\u2029");
const editableReferences = (value: string) => value.replace(/%7B%7B([\w.-]+)%7D%7D/gi, "{{$1}}");
const shell = (value: string) => `'${value.replace(/'/g, "'\\''")}'`;
const singleLine = (value: string) => value.replace(/[\u0000-\u001f\u007f\u2028\u2029]+/g, " ");
const formQuote = (value: string) => `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;

function prepare(request: RequestTemplate, environment: EnvVariable[], placeholders: Set<string>): Plan {
  const req = sanitizeRequestForPersistence(request);
  const variables = new Map(environment.map((row) => [row.key, row]));
  const placeholder = (name: string) => {
    placeholders.add(name);
    return `{{${name}}}`;
  };
  const resolve = (source: string): string => {
    let value = source.replace(/%7B%7B([^%]*?)%7D%7D/gi, "{{$1}}").replace(/%5BREDACTED%5D/gi, "[REDACTED]");
    for (const row of environment)
      if (row.secret && row.value && value.includes(row.value))
        value = value.split(row.value).join(placeholder(row.key));
    value = value.replace(/\{\{\s*([\w.-]+)\s*\}\}|\$\{\s*([\w.-]+)\s*\}/g, (_match, a: string, b: string) => {
      const name = a ?? b;
      const row = variables.get(name);
      return row && !row.secret && row.value ? row.value : placeholder(name);
    });
    for (const row of environment)
      if (row.secret && row.value && value.includes(row.value))
        value = value.split(row.value).join(placeholder(row.key));
    return value.replace(/\[REDACTED\]/g, () => placeholder("REDACTED_VALUE"));
  };
  const params = req.params.filter((row) => row.key).map((row) => [resolve(row.key), resolve(row.value)]);
  let body = req.body_kind === "none" ? "" : resolve(req.body);
  if (req.body_kind === "form") {
    const fields = new URLSearchParams();
    for (const line of body.split(/\r?\n/)) {
      const at = line.indexOf("=");
      if (at >= 0) fields.append(line.slice(0, at), line.slice(at + 1));
    }
    body = editableReferences(fields.toString());
  }
  if (req.body_kind === "graphql" && req.graphql) {
    const graphql = {
      query: resolve(req.graphql.query),
      variables: resolve(req.graphql.variables),
      operation_name: resolve(req.graphql.operation_name),
    };
    body = buildGraphqlBody(graphql);
    if (req.method === "GET") {
      const payload = JSON.parse(body) as Record<string, unknown>;
      for (const [key, value] of Object.entries(payload))
        params.push([key, typeof value === "string" ? value : JSON.stringify(value)]);
      body = "";
    }
  }
  let url = resolve(req.url).split("#")[0];
  if (params.length)
    url +=
      (url.includes("?") ? "&" : "?") +
      params
        .map(([key, value]) => `${encodeURIComponent(key)}=${editableReferences(encodeURIComponent(value))}`)
        .join("&");
  const multipart = req.body_kind === "multipart";
  const headers: [string, string][] = req.headers
    .filter((row) => row.enabled !== false && row.key)
    .filter(
      (row) => !multipart || !["content-type", "content-length", "transfer-encoding"].includes(row.key.toLowerCase()),
    )
    .map((row) => [singleLine(resolve(row.key)), singleLine(resolve(row.value))]);
  const addHeader = (key: string, value: string) => {
    if (!headers.some(([name]) => name.toLowerCase() === key.toLowerCase())) headers.push([key, singleLine(value)]);
  };
  const auth = req.auth;
  if (auth?.kind === "bearer") addHeader("Authorization", `Bearer ${resolve(auth.token)}`);
  if (auth?.kind === "oauth2") addHeader("Authorization", `Bearer ${placeholder("access_token")}`);
  if (auth?.kind === "basic") addHeader("Authorization", `Basic ${placeholder("basic_credentials_base64")}`);
  if (auth?.kind === "apikey") addHeader(singleLine(resolve(auth.api_key)), resolve(auth.api_value));
  const cookies = req.cookies
    .filter((row) => row.enabled !== false && row.name)
    .map((row) => `${singleLine(resolve(row.name))}=${singleLine(resolve(row.value))}`);
  if (cookies.length) addHeader("Cookie", cookies.join("; "));
  if (["json", "graphql"].includes(req.body_kind) && body) addHeader("Content-Type", "application/json");
  if (req.body_kind === "form") addHeader("Content-Type", "application/x-www-form-urlencoded");
  const parts = multipart
    ? req.multipart
        .filter((part) => part.enabled !== false && part.name)
        .map((part) => ({
          name: singleLine(resolve(part.name)),
          file: part.kind === "file",
          value: part.kind === "file" ? placeholder(`file:${singleLine(part.name)}`) : resolve(part.value),
          filename: singleLine(resolve(part.file_name || "file")),
          contentType: singleLine(part.content_type),
        }))
    : [];
  return {
    method: /^[A-Z]+$/.test(req.method) ? req.method : "GET",
    url,
    headers,
    body,
    multipart,
    parts,
    verify: req.tls?.verify !== false,
    credential: Boolean(req.tls?.credentialId),
    timeout: req.timeout_ms / 1000,
  };
}

function curl(plan: Plan): string {
  const lines = [`curl -X ${plan.method} ${shell(plan.url)}`];
  if (!plan.verify) lines.push("  --insecure");
  if (plan.credential) {
    if (plan.verify) lines.push("  --cacert '{{ca_pem}}'");
    lines.push("  --cert '{{client_cert_pem}}' --key '{{client_key_pem}}'");
  }
  for (const [key, value] of plan.headers) lines.push(`  -H ${shell(`${key}: ${value}`)}`);
  if (plan.multipart)
    for (const part of plan.parts) {
      lines.push(
        part.file
          ? `  --form ${shell(`${formQuote(part.name)}=@${formQuote(part.value)}${part.contentType ? `;type=${part.contentType}` : ""}`)}`
          : part.contentType
            ? `  --form ${shell(`${formQuote(part.name)}=${formQuote(part.value)};type=${part.contentType}`)}`
            : `  --form-string ${shell(`${part.name}=${part.value}`)}`,
      );
    }
  else if (plan.body) lines.push(`  --data-raw ${shell(plan.body)}`);
  return lines.join(" \\\n");
}
function fetchCode(plan: Plan): string {
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
function python(plan: Plan): string {
  const lines = ["import requests", "from contextlib import ExitStack", "", "headers = {"];
  for (const [key, value] of plan.headers) lines.push(`    ${quote(key)}: ${quote(value)},`);
  lines.push("}");
  if (new Set(plan.headers.map(([key]) => key.toLowerCase())).size !== plan.headers.length)
    lines.push('raise ValueError("requests는 중복 헤더를 보존하지 않습니다. curl 또는 Go 대상을 사용하세요.")');
  lines.push("with ExitStack() as stack:");
  if (plan.multipart) {
    lines.push("    parts = [");
    for (const part of plan.parts)
      lines.push(
        `        (${quote(part.name)}, (${part.file ? `${quote(part.filename)}, stack.enter_context(open(${quote(part.value)}, "rb"))` : `None, ${quote(part.value)}`}${part.contentType ? `, ${quote(part.contentType)}` : ""})),`,
      );
    lines.push("    ]");
  }
  lines.push(
    `    response = requests.request(${quote(plan.method)}, ${quote(plan.url)}, headers=headers,`,
    `        ${plan.multipart ? "files=parts" : `data=${quote(plan.body)}.encode("utf-8")`}, timeout=${Number.isFinite(plan.timeout) && plan.timeout > 0 ? plan.timeout : 30},`,
    ...(plan.credential ? ["        allow_redirects=False,"] : []),
    `        verify=${!plan.verify ? "False" : plan.credential ? quote("{{ca_pem}}") : "True"}${plan.credential ? `, cert=(${quote("{{client_cert_pem}}")}, ${quote("{{client_key_pem}}")} )` : ""})`,
    "    print(response.text)",
  );
  return lines.join("\n");
}
function go(plan: Plan): string {
  const imports = [
    "io",
    "net/http",
    "os",
    "time",
    ...(plan.multipart
      ? [
          "bytes",
          "mime/multipart",
          ...(plan.parts.length ? ["mime", "net/textproto"] : []),
          ...(plan.parts.some((part) => !part.file) ? ["strings"] : []),
        ]
      : ["strings"]),
    ...(!plan.verify || plan.credential ? ["crypto/tls"] : []),
    ...(plan.credential && plan.verify ? ["crypto/x509"] : []),
  ];
  const lines = [
    "package main",
    "",
    "import (",
    ...imports.map((name) => `  ${quote(name)}`),
    ")",
    "",
    "func must(err error) { if err != nil { panic(err) } }",
    "",
    "func main() {",
  ];
  if (plan.multipart) {
    lines.push("  var body bytes.Buffer", "  writer := multipart.NewWriter(&body)");
    plan.parts.forEach((part, index) => {
      lines.push(
        `  header${index} := make(textproto.MIMEHeader)`,
        `  header${index}.Set("Content-Disposition", mime.FormatMediaType("form-data", map[string]string{"name": ${quote(part.name)}${part.file ? `, "filename": ${quote(part.filename)}` : ""}}))`,
      );
      if (part.contentType || part.file)
        lines.push(`  header${index}.Set("Content-Type", ${quote(part.contentType || "application/octet-stream")})`);
      lines.push(`  part${index}, err := writer.CreatePart(header${index})`, "  must(err)");
      if (part.file)
        lines.push(
          `  file${index}, err := os.Open(${quote(part.value)})`,
          "  must(err)",
          `  defer file${index}.Close()`,
          `  _, err = io.Copy(part${index}, file${index})`,
          "  must(err)",
        );
      else lines.push(`  _, err = io.Copy(part${index}, strings.NewReader(${quote(part.value)}))`, "  must(err)");
    });
    lines.push("  must(writer.Close())");
  } else lines.push(`  body := strings.NewReader(${quote(plan.body)})`);
  lines.push(
    `  req, err := http.NewRequest(${quote(plan.method)}, ${quote(plan.url)}, ${plan.multipart ? "&body" : "body"})`,
    "  must(err)",
  );
  for (const [key, value] of plan.headers) lines.push(`  req.Header.Add(${quote(key)}, ${quote(value)})`);
  if (plan.multipart) lines.push('  req.Header.Set("Content-Type", writer.FormDataContentType())');
  if (!plan.verify || plan.credential) {
    lines.push(`  tlsConfig := &tls.Config{InsecureSkipVerify: ${!plan.verify}}`);
    if (plan.credential) {
      lines.push(
        '  identity, err := tls.LoadX509KeyPair("{{client_cert_pem}}", "{{client_key_pem}}")',
        "  must(err)",
        "  tlsConfig.Certificates = []tls.Certificate{identity}",
      );
      if (plan.verify)
        lines.push(
          "  roots, err := x509.SystemCertPool()",
          "  must(err)",
          '  ca, err := os.ReadFile("{{ca_pem}}")',
          "  must(err)",
          '  if !roots.AppendCertsFromPEM(ca) { panic("CA PEM을 확인하세요") }',
          "  tlsConfig.RootCAs = roots",
        );
    }
    lines.push(
      "  transport := http.DefaultTransport.(*http.Transport).Clone()",
      "  transport.TLSClientConfig = tlsConfig",
    );
  }
  lines.push(
    `  client := &http.Client{Timeout: ${Math.max(1, Math.round(plan.timeout))} * time.Second${!plan.verify || plan.credential ? ", Transport: transport" : ""}${plan.credential ? ", CheckRedirect: func(_ *http.Request, _ []*http.Request) error { return http.ErrUseLastResponse }" : ""}}`,
    "  response, err := client.Do(req)",
    "  must(err)",
    "  defer response.Body.Close()",
    "  _, err = io.Copy(os.Stdout, response.Body)",
    "  must(err)",
    "}",
  );
  return lines.join("\n");
}
function csharp(plan: Plan): string {
  const lines = [
    "using System;",
    "using System.IO;",
    "using System.Net.Http;",
    "using System.Text;",
    ...(plan.credential
      ? [
          "using System.Net.Security;",
          "using System.Security.Cryptography;",
          "using System.Security.Cryptography.X509Certificates;",
        ]
      : []),
    "",
    "using var handler = new HttpClientHandler();",
  ];
  if (!plan.verify)
    lines.push(
      "handler.ServerCertificateCustomValidationCallback = HttpClientHandler.DangerousAcceptAnyServerCertificateValidator;",
    );
  if (plan.credential) {
    lines.push(
      "// .NET 9 이상: PEM 키를 TLS provider가 사용할 수 있는 identity로 읽습니다.",
      "X509Certificate2 LoadIdentity() {",
      '    using var pem = X509Certificate2.CreateFromPemFile("{{client_cert_pem}}", "{{client_key_pem}}");',
      "    var bytes = pem.Export(X509ContentType.Pkcs12);",
      "    try { return X509CertificateLoader.LoadPkcs12(bytes, (string?)null); }",
      "    finally { CryptographicOperations.ZeroMemory(bytes); }",
      "}",
      "using var identity = LoadIdentity();",
      "handler.ClientCertificates.Add(identity);",
      "handler.AllowAutoRedirect = false;",
    );
    if (plan.verify)
      lines.push(
        'using var root = X509Certificate2.CreateFromPem(File.ReadAllText("{{ca_pem}}"));',
        "handler.ServerCertificateCustomValidationCallback = (_, certificate, peerChain, errors) => {",
        "    if (errors == SslPolicyErrors.None) return true;",
        "    if (certificate is null || (errors & SslPolicyErrors.RemoteCertificateNameMismatch) != 0) return false;",
        "    using var chain = new X509Chain();",
        "    chain.ChainPolicy.TrustMode = X509ChainTrustMode.CustomRootTrust;",
        "    chain.ChainPolicy.CustomTrustStore.Add(root);",
        "    if (peerChain != null) foreach (var element in peerChain.ChainElements) chain.ChainPolicy.ExtraStore.Add(element.Certificate);",
        "    return chain.Build(certificate);",
        "};",
      );
  }
  lines.push(
    "using var client = new HttpClient(handler);",
    `using var request = new HttpRequestMessage(new HttpMethod(${quote(plan.method)}), ${quote(plan.url)});`,
  );
  if (plan.multipart) {
    lines.push("var body = new MultipartFormDataContent();");
    plan.parts.forEach((part, index) => {
      lines.push(
        `var part${index} = ${part.file ? `new StreamContent(File.OpenRead(${quote(part.value)}))` : `new StringContent(${quote(part.value)}, Encoding.UTF8)`};`,
      );
      if (part.contentType)
        lines.push(
          `part${index}.Headers.ContentType = new System.Net.Http.Headers.MediaTypeHeaderValue(${quote(part.contentType)});`,
        );
      lines.push(`body.Add(part${index}, ${quote(part.name)}${part.file ? `, ${quote(part.filename)}` : ""});`);
    });
    lines.push("request.Content = body;");
  } else if (plan.body || plan.headers.some(([key]) => key.toLowerCase().startsWith("content-"))) {
    lines.push(
      `request.Content = new StringContent(${quote(plan.body)}, Encoding.UTF8);`,
      'request.Content.Headers.Remove("Content-Type");',
    );
  }
  for (const [key, value] of plan.headers) {
    if (key.toLowerCase().startsWith("content-"))
      lines.push(`request.Content!.Headers.TryAddWithoutValidation(${quote(key)}, ${quote(value)});`);
    else lines.push(`request.Headers.TryAddWithoutValidation(${quote(key)}, ${quote(value)});`);
  }
  lines.push(
    "using var response = await client.SendAsync(request);",
    "Console.WriteLine(await response.Content.ReadAsStringAsync());",
  );
  return lines.join("\n");
}
export function generateCode(
  target: CodeTarget,
  request: RequestTemplate,
  environment: EnvVariable[],
): { code: string; placeholders: string[] } {
  const placeholders = new Set<string>();
  const plan = prepare(request, environment, placeholders);
  const comments: string[] = [];
  if (plan.credential && target !== "fetch") {
    if (plan.verify) placeholders.add("ca_pem");
    placeholders.add("client_cert_pem");
    placeholders.add("client_key_pem");
    comments.push(
      "TLS 자격 증명은 내보내지 않습니다. PEM 파일 경로를 채우고 사용하지 않는 CA 또는 클라이언트 인증서 설정은 제거하세요.",
    );
  }
  if (!plan.verify) comments.push("인증서 검증 꺼짐");
  if (target === "fetch") comments.push("브라우저의 CORS·금지 헤더·쿠키 정책이 적용됩니다.");
  if (plan.headers.some(([key]) => key.toLowerCase() === "authorization") && request.auth?.kind === "basic")
    comments.push("basic_credentials_base64에는 사용자 이름:비밀번호의 UTF-8 base64를 넣으세요.");
  const generator = { curl, fetch: fetchCode, python, go, csharp }[target];
  const code = generator(plan);
  const names = [...placeholders].sort();
  if (names.length) comments.unshift(`채워 넣을 값: ${names.map((name) => `{{${name}}}`).join(", ")}`);
  const prefix = target === "curl" || target === "python" ? "#" : "//";
  return { code: [...comments.map((line) => `${prefix} ${singleLine(line)}`), code].join("\n"), placeholders: names };
}
