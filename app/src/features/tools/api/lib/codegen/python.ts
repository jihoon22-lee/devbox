import { type Plan, quote } from "./plan";

export function python(plan: Plan): string {
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
