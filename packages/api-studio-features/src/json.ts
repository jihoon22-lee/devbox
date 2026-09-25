import type { JsonValue } from "./generated/serde_json/JsonValue";

/** Apply the JSON wire representation before crossing a typed native boundary. */
export function toJsonValue(value: unknown): JsonValue {
  const encoded = JSON.stringify(value);
  if (encoded === undefined) throw new Error("JSON value is required");
  return JSON.parse(encoded) as JsonValue;
}
