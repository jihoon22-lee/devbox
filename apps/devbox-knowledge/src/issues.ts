import { activityMessages } from "@devbox/knowledge-features/activity/issues";
import { notesMessages } from "@devbox/knowledge-features/notes/issues";
import { searchMessages } from "@devbox/knowledge-features/search/issues";
import { setupMessages } from "@devbox/knowledge-features/setup/issues";
import { commandsMessages } from "@devbox/knowledge-features/commands/issues";
import type { Component } from "@devbox/knowledge-features/transport";
const catalogs: Record<Component, Readonly<Record<string, string>>> = {
  "knowledge.activity": activityMessages,
  "knowledge.notes": notesMessages,
  "knowledge.search": searchMessages,
  "knowledge.search-settings": searchMessages,
  "knowledge.opener": searchMessages,
  "knowledge.setup": setupMessages,
  "knowledge.commands": commandsMessages,
};
export function issueError(issue: unknown, component: Component = "knowledge.setup"): Error {
  const messages = catalogs[component];
  const known = typeof issue === "string" && Object.prototype.hasOwnProperty.call(messages, issue);
  const error = new Error(known ? messages[issue] : messages.unavailable);
  if (known) error.name = issue;
  return error;
}
