import { RPC_ERROR_CODES } from "./gen/rpc";
import { messageFor } from "./messages";

test("all generated codes have Korean messages and uncertain results stay explicit", () => {
  for (const code of RPC_ERROR_CODES) expect(messageFor(code).title).toMatch(/[가-힣]/);
  expect(messageFor("connection_lost").title).toContain("결과");
  expect(messageFor("projects.path_invalid", { path: "/hello" }).title).toContain("/hello");
  expect(messageFor("cancelled").title).not.toContain("되돌");
});
