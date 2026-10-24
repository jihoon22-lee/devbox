import type { RequestHeader, RequestTemplate } from "./template";

/** Structured rows are decoded data. An unmarked legacy body is literal name=value lines. */
export function formFields(request: RequestTemplate): RequestHeader[] {
  if (request.form_fields) return request.form_fields;
  return request.body
    .split(/\r?\n/)
    .filter((line) => line.trim() && !line.trimStart().startsWith("#"))
    .map((line) => {
      const at = line.indexOf("=");
      return { key: (at < 0 ? line : line.slice(0, at)).trim(), value: at < 0 ? "" : line.slice(at + 1) };
    });
}

/** Encoded display text cannot introduce delimiters from a field's contents. */
export const formText = (fields: RequestHeader[]) =>
  fields
    .filter((field) => field.enabled !== false)
    .map(({ key, value }) => `${encodeURIComponent(key)}=${encodeURIComponent(value)}`)
    .join("\n");

export function importedForm(fields: RequestHeader[]): Pick<RequestTemplate, "body_kind" | "body" | "form_fields"> {
  return { body_kind: "form", body: formText(fields), form_fields: fields };
}

/** Only wire-format producers call this: delimiters/plus are decoded exactly once. */
export const wireForm = (body: string) =>
  importedForm(Array.from(new URLSearchParams(body), ([key, value]) => ({ key, value })));
