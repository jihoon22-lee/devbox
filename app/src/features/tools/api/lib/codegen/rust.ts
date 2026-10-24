import type { Plan } from "./plan";

/** Rust 문자열 리터럴. JSON 이스케이프(`\uXXXX`·`\b`)는 Rust에서 쓸 수 없어 따로 만든다. */
export function rustString(value: string): string {
  let out = '"';
  for (const character of value.replace(/[\uD800-\uDFFF]/gu, "�")) {
    const code = character.codePointAt(0) ?? 0;
    if (character === "\\") out += "\\\\";
    else if (character === '"') out += '\\"';
    else if (character === "\n") out += "\\n";
    else if (character === "\r") out += "\\r";
    else if (character === "\t") out += "\\t";
    else if (code < 0x20 || code === 0x7f || code === 0x2028 || code === 0x2029) out += `\\u{${code.toString(16)}}`;
    else out += character;
  }
  return `${out}"`;
}

/** Rust reqwest(blocking). `RequestBuilder::header`는 같은 이름을 덧붙이므로 중복 헤더가 그대로 간다. */
export function rust(plan: Plan): string {
  const lines = [
    '// Cargo.toml: reqwest = { version = "0.12", features = ["blocking", "multipart", "rustls-tls"] }',
    "use std::time::Duration;",
    "",
    "fn main() -> Result<(), Box<dyn std::error::Error>> {",
  ];
  const timeout = Number.isFinite(plan.timeout) && plan.timeout > 0 ? plan.timeout : 30;
  lines.push(
    "    let client = reqwest::blocking::Client::builder()",
    `        .timeout(Duration::from_secs_f64(${Number.isInteger(timeout) ? `${timeout}.0` : timeout}))`,
  );
  if (!plan.verify) lines.push("        .danger_accept_invalid_certs(true)");
  if (plan.credential) {
    if (plan.verify)
      lines.push('        .add_root_certificate(reqwest::Certificate::from_pem(&std::fs::read("{{ca_pem}}")?)?)');
    lines.push(
      '        .identity(reqwest::Identity::from_pem(&[std::fs::read("{{client_cert_pem}}")?, std::fs::read("{{client_key_pem}}")?].concat())?)',
      "        .redirect(reqwest::redirect::Policy::none())",
    );
  }
  lines.push("        .build()?;");
  if (plan.multipart) {
    lines.push("    let form = reqwest::blocking::multipart::Form::new()");
    for (const part of plan.parts) {
      const mime = part.contentType ? `.mime_str(${rustString(part.contentType)})?` : "";
      lines.push(
        part.file
          ? `        .part(${rustString(part.name)}, reqwest::blocking::multipart::Part::file(${rustString(part.value)})?.file_name(${rustString(part.filename)})${mime})`
          : `        .part(${rustString(part.name)}, reqwest::blocking::multipart::Part::text(${rustString(part.value)})${mime})`,
      );
    }
    lines.push(`${lines.pop() ?? ""};`);
  }
  lines.push(
    "    let response = client",
    `        .request(reqwest::Method::from_bytes(${rustString(plan.method)}.as_bytes())?, ${rustString(plan.url)})`,
  );
  for (const [key, value] of plan.headers) lines.push(`        .header(${rustString(key)}, ${rustString(value)})`);
  if (plan.multipart) lines.push("        .multipart(form)");
  else if (plan.body) lines.push(`        .body(${rustString(plan.body)})`);
  lines.push("        .send()?;", '    println!("{}", response.text()?);', "    Ok(())", "}");
  return lines.join("\n");
}
