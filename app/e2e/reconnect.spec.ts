import { join } from "node:path";
import { decodeFrame } from "../src/rpc/frame";
import { expect, test } from "./support";

test("new daemon identity resets subscriptions and queries while retaining projects", async ({ page, stack }) => {
  const welcomes: string[] = [];
  const sentAfterWelcome: { type?: string; method?: string; topic?: string }[] = [];
  page.on("websocket", (socket) => {
    if (socket.url().includes("observer=reset")) return;
    socket.on("framereceived", ({ payload }) => {
      if (typeof payload === "string") return;
      const message = JSON.parse(new TextDecoder().decode(decodeFrame(payload).body));
      if (message.type === "welcome") {
        welcomes.push(message.daemonId);
        sentAfterWelcome.length = 0;
      }
    });
    socket.on("framesent", ({ payload }) => {
      if (typeof payload !== "string")
        sentAfterWelcome.push(JSON.parse(new TextDecoder().decode(decodeFrame(payload).body)));
    });
  });
  await page.goto("/");
  await expect(page.getByText("데몬 연결됨")).toBeVisible();
  await page.keyboard.press("Control+Shift+O");
  await page.getByRole("button", { name: "프로젝트 추가", exact: true }).click();
  await page.getByRole("dialog", { name: "프로젝트 추가", exact: true }).getByRole("button", { name: /alpha/ }).click();
  await expect(page.getByRole("dialog", { name: "프로젝트 추가", exact: true })).toBeHidden();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("button", { name: "프로젝트 전환" })).toHaveText(/alpha/);
  await page.keyboard.press("Control+Shift+O");
  // A second real client observes the public reset callback, independently of UI refreshes.
  await page.evaluate(
    async ({ gatewayUrl, instance }) => {
      const load = (path: string) => import(/* @vite-ignore */ path);
      const [{ RpcClient }, { wsTransport }] = await Promise.all([
        load("/src/rpc/client.ts"),
        load("/src/rpc/transport-ws.ts"),
      ]);
      const client = new RpcClient(wsTransport(`${gatewayUrl}&observer=reset`), {
        version: "1.0.0-dev",
        instance,
        client: "test",
      });
      const observation = { client, resets: 0 };
      client.onReset(() => observation.resets++);
      Object.assign(window, { resetObservation: observation });
      client.start();
    },
    { gatewayUrl: stack.gatewayUrl, instance: stack.instance },
  );
  const observedReset = () =>
    page.evaluate(() => {
      const observation = (
        window as unknown as { resetObservation: { resets: number; client: { state: string; daemonId?: string } } }
      ).resetObservation;
      return { resets: observation.resets, state: observation.client.state, daemonId: observation.client.daemonId };
    });
  await expect.poll(async () => (await observedReset()).state).toBe("ready");
  const before = await stack.hello();
  expect(welcomes.at(-1)).toBe(before);
  await stack.stopDaemon();
  await expect(page.getByText(/다시 연결하고 있습니다/)).toBeVisible();
  await stack.startDaemon();
  const after = await stack.hello();
  expect(after).not.toBe(before);
  await expect.poll(observedReset).toEqual({ resets: 1, state: "ready", daemonId: after });
  await expect(page.getByText("데몬 연결됨")).toBeVisible({ timeout: 15_000 });
  await expect(page.getByText(/다시 연결하고 있습니다/)).toHaveCount(0);
  await expect.poll(() => welcomes.at(-1)).toBe(after);
  // Observe actual browser traffic: the new daemon must cause subscription and query restoration.
  await expect
    .poll(() =>
      sentAfterWelcome.some((message) => message.type === "subscribe" && message.topic === "projects.changed"),
    )
    .toBe(true);
  await expect
    .poll(() => sentAfterWelcome.some((message) => message.type === "request" && message.method === "projects.list"))
    .toBe(true);
  await expect(page.getByText(join(stack.home, "projects/alpha"), { exact: true })).toBeVisible();
  // A post-restart mutation also proves the restored subscription remains usable.
  await page.getByRole("button", { name: "alpha 제거", exact: true }).click();
  await page.getByRole("button", { name: "프로젝트 제거", exact: true }).click();
  await expect(page.getByText(join(stack.home, "projects/alpha"), { exact: true })).toHaveCount(0);
});
