import contract from "../component-errors.json";
import type { Component } from "@devbox/api-studio-features/transport";
/** Called only after the enclosing operation's native provenance is verified. */
export function componentFailure(component: Component, value: unknown): Error {
  const messages = contract.components[component as keyof typeof contract.components];
  if (value && typeof value === "object" && !Array.isArray(value) && Object.keys(value).join(",") === "issue") {
    const issue = (value as { issue: unknown }).issue;
    if (typeof issue === "string" && issue !== "component_unavailable" && messages?.includes(issue)) return new Error(issue);
  }
  return new Error("작업을 완료하지 못했습니다.");
}
