import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test } from "vitest";
import { RpcError } from "../../rpc/client";
import { RpcProvider } from "../../rpc/RpcProvider";
import { NotesWire } from "../notes/test-wire";
import { RulesEditor } from "./RulesEditor";

afterEach(cleanup);

test("invalid_rules points at the second submitted pattern without discarding drafts", async () => {
  const wire = new NotesWire();
  wire.handlers["activity.rules"] = () => ({
    rev: "1",
    idleThresholdMs: 300000,
    rules: {
      maskAllTitles: false,
      excludedProcesses: [],
      excludedTitlePatterns: ["secret", "["],
      redactTitlePatterns: [],
    },
  });
  wire.handlers["activity.set_rules"] = () => {
    throw new RpcError("activity.invalid_rules", {
      problems: [{ field: "excludedTitlePatterns", index: 1, problem: "syntax" }],
    });
  };
  render(
    <RpcProvider client={wire.client}>
      <RulesEditor />
    </RpcProvider>,
  );
  const second = await screen.findByRole("textbox", { name: "제목 제외 정규식 2" });
  await userEvent.click(screen.getByRole("button", { name: "개인정보 규칙 저장" }));
  expect(await screen.findByText("정규식 문법을 확인하세요.")).toBeVisible();
  expect(second).toHaveAttribute("aria-invalid", "true");
  expect(within(second.parentElement!).getByRole("alert")).toHaveTextContent("정규식 문법");
  expect(second).toHaveValue("[");
});
