import { render, screen } from "@testing-library/react";
import type { Methods } from "../../rpc/gen/rpc";
import { ProjectTrustDialog } from "./ProjectTrustDialog";
import { trustPreview } from "./trust-fixtures";

test("scope two approval displays every executable input and schedule link with visible controls", () => {
  const common = {
    command: "echo \u202erun",
    cwd: "sub\u200fdir",
    env: { "MODE\u2066": "value\u2069", TOKEN: "${secret:API_TOKEN}" },
    target: "windows",
    dependsOn: ["first\u200e", "second"],
    worktree: "agent\u061cwork",
    secrets: ["API_TOKEN\u202c"],
  };
  const preview = trustPreview("a", {
    scopeVersion: 2,
    runDefinitions: [
      { kind: "task", id: "task\u202a", ...common },
      {
        kind: "service",
        id: "service\u202b",
        ...common,
        restart: "onFailure",
        restartSec: 2,
        startLimit: { burst: 3, intervalSec: 60 },
        portsHint: [3000],
        readyTimeoutSec: 20,
      },
      {
        kind: "schedule",
        id: "night\u200b",
        task: "task\u202a",
        cron: "0 9 * * *",
        tz: "Asia/Seoul",
        missed: "runOnce",
      },
    ],
  }) as Methods["projects.trust_preview"]["result"];
  const original = JSON.stringify(preview);
  render(
    <ProjectTrustDialog
      preview={preview}
      pending={false}
      notice=""
      onCancel={vi.fn()}
      onRetry={vi.fn()}
      onApprove={vi.fn()}
    />,
  );
  const section = screen.getByRole("region", { name: "프로젝트 실행 정의" });
  const text = section.textContent ?? "";
  for (const value of [
    "task\\u202A",
    "service\\u202B",
    "echo \\u202Erun",
    "sub\\u200Fdir",
    "MODE\\u2066",
    "value\\u2069",
    "Windows",
    "first\\u200E → second",
    "agent\\u061Cwork",
    "API_TOKEN\\u202C",
    "${secret:API_TOKEN}",
    "night\\u200B",
    "0 9 * * *",
    "Asia/Seoul",
    "3000",
    "20",
  ])
    expect(text).toContain(value);
  expect(text).not.toMatch(/\p{Bidi_Control}|\u200b/u);
  expect(JSON.stringify(preview)).toBe(original);
});
