import { RPC_ERROR_CODES } from "./gen/rpc";
import { messageFor } from "./messages";

test("all generated codes have Korean messages and uncertain results stay explicit", () => {
  for (const code of RPC_ERROR_CODES) expect(messageFor(code).title).toMatch(/[가-힣]/);
  expect(messageFor("connection_lost").title).toContain("결과");
  expect(messageFor("projects.path_invalid", { path: "/hello" }).title).toContain("/hello");
  expect(messageFor("cancelled").title).not.toContain("되돌");
});

test("agent errors use actual generated file details and terminal codes", () => {
  expect(messageFor("agents.base_dirty", { files: ["src/main.rs"] }).action).toContain("src/main.rs");
  expect(messageFor("agents.conflict", { files: ["app.tsx"] }).action).toContain("app.tsx");
  expect(messageFor("agents.conflict", { files: [1, null] }).action).not.toContain("null");
  expect(messageFor("terminal.cwd_invalid").action).toContain("WSL");
  expect(messageFor("terminal.title_too_long", { limit: 256 }).action).toContain("256");
});
