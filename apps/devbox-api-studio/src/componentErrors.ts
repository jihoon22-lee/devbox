import { apiMessages, webhookMessages, transformMessages } from "@devbox/api-studio-features/issues/catalog";
import type { Component } from "@devbox/api-studio-features/transport";
const catalogs: Record<Component, Readonly<Record<string, string>>> = {
  "api-studio.api": apiMessages,
  "api-studio.webhooks": webhookMessages,
  "api-studio.transforms": transformMessages,
};
/** Only declared native codes can select a message after provenance validation. */
export function componentFailure(component: Component, value: unknown): Error {
  const messages = catalogs[component];
  if (value && typeof value === "object" && !Array.isArray(value) && Object.keys(value).join(",") === "issue") {
    const issue = (value as { issue: unknown }).issue;
    if (typeof issue === "string" && Object.prototype.hasOwnProperty.call(messages, issue)) {
      const error = new Error(messages[issue]);
      error.name = issue;
      return error;
    }
  }
  return new Error(messages.unavailable);
}
