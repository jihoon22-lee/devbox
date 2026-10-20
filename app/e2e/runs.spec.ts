import { expect, test } from "./support";

test.use({ runtimeEnabled: true });

test("define echo task, start, stream a line and display its exit code", async ({ page, stack }) => {
  const projectId = await stack.gammaProjectId();
  await page.addInitScript(
    (id) => localStorage.setItem("devbox.currentProject", JSON.stringify({ state: { id }, version: 0 })),
    projectId,
  );
  await page.goto(`/runs?gateway=${encodeURIComponent(stack.gatewayUrl)}`);
  await page.getByRole("button", { name: "작업 추가 ▾" }).click();
  await page.getByRole("menuitem", { name: "직접 추가" }).click();
  await page.getByLabel("작업 이름 (식별자)").fill("owned-echo");
  await page.getByRole("textbox", { name: "명령", exact: true }).fill("echo owned-e2e-line");
  await page.getByRole("button", { name: "저장", exact: true }).click();
  await expect(page.getByRole("heading", { name: "작업 수정" })).toBeVisible();
  await page.getByRole("button", { name: "닫기", exact: true }).last().click();
  await page.getByRole("button", { name: "owned-echo 시작", exact: true }).click();
  await expect(page.getByText("owned-e2e-line", { exact: true })).toBeVisible();
  await expect(page.getByText("종료 코드 0", { exact: true })).toBeVisible();
  const list = await stack.rpc("runtime.list", { projectId, cursor: null });
  expect(list.runs.find((run) => run.defId === "owned-echo")?.exit?.code).toBe(0);
});

test("start owned HTTP service, show port, stop and undo with the same run ID", async ({ page, stack }) => {
  const projectId = await stack.gammaProjectId();
  await stack.rpc("runtime.define", {
    projectId,
    saveTo: "thisPc",
    def: {
      kind: "service",
      id: "owned-http",
      command: "python3 -m http.server $DEVBOX_PORT --bind 127.0.0.1",
      cwd: null,
      env: {},
      target: "linux",
      dependsOn: [],
      worktree: null,
      secrets: [],
      restart: "no",
      restartSec: 1,
      startLimit: { burst: 3, intervalSec: 60 },
      portsHint: [],
      readyTimeoutSec: 10,
    },
  });
  await page.addInitScript(
    (id) => localStorage.setItem("devbox.currentProject", JSON.stringify({ state: { id }, version: 0 })),
    projectId,
  );
  await page.goto(`/runs?gateway=${encodeURIComponent(stack.gatewayUrl)}`);
  await page.getByRole("button", { name: "owned-http 시작" }).click();
  await expect(page.getByRole("button", { name: /포트 \d+ 브라우저로 열기/ })).toBeVisible();
  const before = (await stack.rpc("runtime.list", { projectId, cursor: null })).runs.find(
    (run) => run.defId === "owned-http",
  )!;
  expect(before.id).toBeTruthy();
  await page.getByRole("button", { name: "owned-http 중지" }).click();
  await page.getByRole("button", { name: "되돌리기", exact: true }).click();
  await expect(page.getByText("owned-http 다시 시작됨", { exact: true })).toBeVisible();
  const after = await stack.rpc("runtime.get", { runId: before.id });
  expect(after.run.id).toBe(before.id);
  expect(after.run.state).toBe("running");
});
