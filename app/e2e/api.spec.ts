import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { ApiFixture } from "./api-fixture-server";
import { expect, test } from "./support";

const personal = { kind: "personal" } as const;
const file = (name: string, request: Record<string, unknown>, captures: unknown[] = []) =>
  `${JSON.stringify(
    {
      version: 1,
      name,
      request: {
        query: [],
        headers: [],
        cookies: [],
        body: { kind: "none" },
        auth: { kind: "none" },
        tls: { verify: true, caPath: null, clientCertPath: null, clientKeyPath: null },
        timeoutMs: 10_000,
        followRedirects: true,
        ...request,
      },
      assertions: [],
      captures,
    },
    null,
    2,
  )}\n`;

test("paste a cURL command, import it, send and show status, body and origin", async ({ page, stack }) => {
  const fixture = await new ApiFixture().start();
  try {
    const path = `/hello-${Date.now()}`;
    await page.goto(`/tools?gateway=${encodeURIComponent(stack.gatewayUrl)}`);
    await page.getByRole("button", { name: "가져오기", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "가져오기" });
    await dialog.getByLabel("가져올 내용").fill(`curl '${fixture.url}${path}' -H 'X-Trace: e2e'`);
    await dialog.getByRole("button", { name: "미리보기" }).click();
    await expect(dialog.getByRole("list", { name: "만들 파일" })).toContainText(`GET -hello-`);
    await dialog.getByRole("button", { name: "가져오기", exact: true }).click();
    await expect(dialog.getByText("요청 1개를 가져왔습니다.")).toBeVisible();
    await dialog.getByRole("button", { name: "닫기", exact: true }).first().click();
    await page
      .getByRole("tree", { name: "개인 컬렉션" })
      .getByRole("treeitem", { name: /GET -hello-/ })
      .click();
    await page.getByRole("button", { name: "보내기" }).click();
    await expect(page.getByText("200 OK", { exact: true })).toBeVisible();
    await expect(page.locator("pre").filter({ hasText: '"hello": "world"' })).toBeVisible();
    const origin = page.getByRole("group", { name: "응답을 만든 요청" });
    await expect(origin).toContainText("GET");
    await expect(origin).toContainText(`${fixture.url}${path}`);
    expect(fixture.received.find((item) => item.path === path)?.trace).toBe("e2e");
    // End may precede the durable history record. Wait for that record, without resending.
    await expect
      .poll(async () => {
        const history = await stack.rpc("api.history", { before: null, limit: 16, origin: null });
        return history.items.some((entry) => entry.url === `${fixture.url}${path}` && entry.outcome === "ok");
      })
      .toBe(true);
    await page.getByRole("button", { name: "노트에 붙이기", exact: true }).click();
    const append = page.getByRole("dialog", { name: "응답을 노트에 붙이기" });
    await expect(append.getByLabel("붙일 곳")).toHaveValue("daily");
    await append.getByRole("button", { name: "붙이기", exact: true }).click();
    await expect(page.getByText(/daily\/.*에 붙였습니다\./)).toBeVisible();
    const daily = join(stack.home, "notes", "daily");
    await expect
      .poll(() =>
        readdirSync(daily)
          .filter((name) => name.endsWith(".md"))
          .map((name) => readFileSync(join(daily, name), "utf8"))
          .join("\n"),
      )
      .toContain('"hello":"world"');
    expect(fixture.received.filter((item) => item.path === path)).toHaveLength(1);
  } finally {
    await fixture.stop();
  }
});

test("runner sends the token captured by login to the next request", async ({ page, stack }) => {
  const fixture = await new ApiFixture().start();
  try {
    const capture = { id: "c1", enabled: true, variable: "session", source: "jsonPath", target: "$.session" };
    await stack.rpc("api.collection_write", {
      root: personal,
      path: "1 login.request.json",
      text: file("login", { method: "POST", url: `${fixture.url}/login` }, [{ ...capture, target: "$.token" }]),
      expect: { kind: "absent" },
    });
    await stack.rpc("api.collection_write", {
      root: personal,
      path: "2 me.request.json",
      text: file("me", {
        method: "GET",
        url: `${fixture.url}/me`,
        headers: [{ name: "X-Session", value: "{{session}}", enabled: true }],
      }),
      expect: { kind: "absent" },
    });
    // 이전 실행 값이 남아 있어도 이번 로그인 값을 써야 한다: 두 번 실행한다.
    await page.goto(`/tools?gateway=${encodeURIComponent(stack.gatewayUrl)}&view=api&tool=runner&root=personal`);
    const workspace = page.getByRole("region", { name: "본문", exact: true });
    const plan = workspace.getByRole("list", { name: "실행 순서" });
    await expect(plan.getByRole("listitem", { name: "2. me" })).toContainText("session ← login");
    for (let round = 1; round <= 2; round++) {
      await workspace.getByRole("button", { name: "실행", exact: true }).click();
      await expect(workspace.getByText("통과 2 · 실패 0 · 오류 0 · 건너뜀 0")).toBeVisible();
      const results = workspace.getByRole("table", { name: "실행 결과" });
      await expect(results.getByRole("row", { name: "me" })).toContainText("session: login");
      expect(fixture.issued).toHaveLength(round);
      expect(fixture.received.filter((item) => item.path === "/me").at(-1)?.session).toBe(fixture.issued.at(-1));
    }
  } finally {
    await fixture.stop();
  }
});
