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
    readable: true,
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

const NONE = { maskAllTitles: false, excludedProcesses: [], excludedTitlePatterns: [], redactTitlePatterns: [] };

test("rules the daemon could not read say why recording is off and cannot be saved over", async () => {
  const wire = new NotesWire();
  // `unreadable()` lists: saving them would replace what the user wrote in the file.
  wire.handlers["activity.rules"] = () => ({
    rev: "1",
    idleThresholdMs: 300000,
    readable: false,
    rules: { ...NONE, maskAllTitles: true },
  });
  render(
    <RpcProvider client={wire.client}>
      <RulesEditor />
    </RpcProvider>,
  );
  expect(
    await screen.findByText(
      "설정 파일의 [activity]를 읽지 못해 활동 기록을 껐습니다. 여기서 저장하면 파일에 적은 규칙을 덮어쓰므로 저장할 수 없습니다. 그 파일을 직접 고친 뒤 설정 → 앱 진단 → [다시 설정]을 눌러 주세요.",
    ),
  ).toBeVisible();
  expect(screen.getByRole("button", { name: "개인정보 규칙 저장" })).toBeDisabled();
});

test("a save the daemon refused names the file and what to do", async () => {
  const wire = new NotesWire();
  wire.handlers["activity.rules"] = () => ({ rev: "1", idleThresholdMs: 300000, readable: true, rules: NONE });
  wire.handlers["activity.set_rules"] = () => {
    throw new RpcError("activity.config_invalid", { file: "/fixture/devbox/config.local.toml" });
  };
  render(
    <RpcProvider client={wire.client}>
      <RulesEditor />
    </RpcProvider>,
  );
  await userEvent.click(await screen.findByRole("button", { name: "개인정보 규칙 저장" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "/fixture/devbox/config.local.toml의 [activity]를 읽지 못해 규칙을 저장하지 않았습니다. 그 파일의 [activity]를 직접 고친 뒤 설정 → 앱 진단 → [다시 설정]을 눌러 주세요.",
  );
});
