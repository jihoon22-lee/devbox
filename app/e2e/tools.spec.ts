import { expect, test } from "./support";

test("webhook reception opens an API request and its body in the JSON formatter", async ({ page, stack }) => {
  await page.goto(`/tools?gateway=${encodeURIComponent(stack.gatewayUrl)}&view=webhooks`);
  const content = page.getByRole("region", { name: "본문", exact: true });
  const panel = content.getByRole("region", { name: "웹훅", exact: true });
  await panel.getByRole("spinbutton", { name: "포트" }).fill("0");
  await expect(panel.getByRole("checkbox", { name: "LAN에서 받기" })).not.toBeChecked();
  let listenerId: string | undefined;
  try {
    await panel.getByRole("button", { name: "수신 시작" }).click();
    await expect(panel.getByRole("button", { name: "수신 중지" })).toBeVisible();
    const listeners = await stack.rpc("webhooks.listeners", {});
    expect(listeners.items).toHaveLength(1);
    const listener = listeners.items[0]!;
    listenerId = listener.id;
    expect(listener.lan).toBe(false);
    expect(listener.running).toBe(true);
    expect(listener.port).toBeGreaterThan(0);
    const origin = `http://127.0.0.1:${listener.port}`;
    const body = '{"event":"owned-webhook","payload":{"count":3,"text":"한글"}}';
    // Only the listener started by this isolated Stack is a network target.
    const response = await fetch(`${origin}/formatter-fixture?source=tools`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body,
      signal: AbortSignal.timeout(5000),
      redirect: "error",
    });
    expect(response.ok).toBe(true);
    await response.arrayBuffer();
    await panel
      .getByRole("list", { name: "받은 요청" })
      .getByRole("button", { name: /POST \/formatter-fixture/ })
      .click();
    const detail = panel.getByRole("region", { name: "수신 상세" });
    await expect(detail.getByRole("textbox", { name: "수신 본문" })).toHaveText(body);
    await detail.getByRole("button", { name: "API 요청으로 열기" }).click();
    await expect(page.getByRole("textbox", { name: "URL", exact: true })).toHaveValue(
      `${origin}/formatter-fixture?source=tools`,
    );
    await expect(page.getByRole("combobox", { name: "메서드" })).toHaveValue("POST");
    await page.getByRole("tab", { name: "본문", exact: true }).click();
    const editor = page.getByRole("textbox", { name: "텍스트 본문" });
    await expect(editor).toHaveText(body);
    const receivedBody = await editor.innerText();
    await page.getByRole("button", { name: "변환 도구 열기" }).click();
    const transforms = content.getByRole("region", { name: "변환 도구", exact: true });
    await transforms.getByRole("combobox", { name: "변환 도구" }).selectOption("json-format");
    await transforms.getByRole("textbox", { name: "입력", exact: true }).fill(receivedBody);
    await expect(transforms.getByRole("region", { name: "출력", exact: true })).toHaveText(
      JSON.stringify(JSON.parse(body), null, 2),
    );
    // Opening the request and formatting it must not implicitly replay the POST.
    expect((await stack.rpc("webhooks.history", { listenerId, before: null, limit: 10 })).items).toHaveLength(1);
  } finally {
    if (listenerId) await stack.rpc("webhooks.stop", { id: listenerId });
  }
});
