import { type Plan, quote } from "./plan";

export function go(plan: Plan): string {
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
