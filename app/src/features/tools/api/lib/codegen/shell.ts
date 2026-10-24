import { formQuote, type Plan, shell } from "./plan";

export function curl(plan: Plan): string {
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
