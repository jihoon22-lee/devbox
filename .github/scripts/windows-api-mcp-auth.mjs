import assert from "node:assert/strict";
import path from "node:path";
import { createServer } from "node:http";
import { once } from "node:events";
import { mkdir, writeFile, copyFile, readFile } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import { allWindowsProcesses } from "./windows-packaged-smoke.mjs";
import {
  requireApiContext,
  button,
  textbox,
  select,
  scenario,
  until,
  expectText,
  bodyText,
} from "./windows-api-user-flow-actions.mjs";

export const discover = (id) => ({
  jsonrpc: "2.0",
  id,
  result: {
    resultType: "complete",
    supportedVersions: ["2026-07-28"],
    capabilities: { tools: {} },
    ttlMs: 60000,
    cacheScope: "public",
    _meta: { "io.modelcontextprotocol/serverInfo": { name: "Owned OAuth fixture", version: "1" } },
  },
});
export function toolReply(message) {
  if (message.method === "server/discover") return discover(message.id);
  if (message.method === "tools/list")
    return {
      jsonrpc: "2.0",
      id: message.id,
      result: {
        resultType: "complete",
        ttlMs: 0,
        cacheScope: "private",
        tools: [
          {
            name: "synthetic_echo",
            description: "No side effects",
            inputSchema: { type: "object", properties: {}, additionalProperties: false },
          },
        ],
      },
    };
  return {
    jsonrpc: "2.0",
    id: message.id,
    result: { resultType: "complete", content: [{ type: "text", text: "synthetic tool result" }] },
  };
}
export async function createOAuthMcpFixture() {
  const calls = [],
    revocations = [];
  const server = createServer(async (request, response) => {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    const body = Buffer.concat(chunks).toString("utf8");
    if (request.url === "/revoke") {
      revocations.push(Object.fromEntries(new URLSearchParams(body)));
      response.writeHead(200).end();
      return;
    }
    const message = JSON.parse(body);
    calls.push({ method: message.method, authorization: request.headers.authorization });
    response.setHeader("Content-Type", "application/json");
    response.end(JSON.stringify(toolReply(message)));
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const base = `http://127.0.0.1:${server.address().port}`;
  return {
    base,
    endpoint: `${base}/mcp`,
    calls,
    revocations,
    close: async () => {
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    },
  };
}

export const SCENARIO_IDS = Object.freeze(["AUTH-01", "AUTH-02"]);
export async function reseedOAuthGrantFixture(context, file, body) {
  await writeFile(file, body);
  // The native OAuth owner caches its loaded grant store. Fixture replacement
  // is preparation for a new process, never an external live-cache mutation.
  await context.restart();
}

export async function run(context) {
  requireApiContext(context);
  const fixture = await createOAuthMcpFixture();
  const results = [];
  const tokenA = `synthetic-A-${randomUUID()}`,
    tokenB = `synthetic-B-${randomUUID()}`;
  const directory = path.join(context.namespace, "api", "oauth"),
    file = path.join(directory, "mcp-grants.json");
  await mkdir(directory, { recursive: true });
  const grant = async (id, clientId, token) => ({
    grantId: id,
    issuer: `${fixture.base}/issuer`,
    resource: fixture.endpoint,
    clientId,
    scopes: ["read"],
    accessToken: await context.nativeCall("plugin:api-studio|api", "seal_secret", { value: token }),
    refreshToken: null,
    expiresAtMs: Date.now() + 3600000,
    authorizationEndpoint: `${fixture.base}/authorize`,
    tokenEndpoint: `${fixture.base}/token`,
    revocationEndpoint: `${fixture.base}/revoke`,
    authorizationResponseIssParameterSupported: false,
  });
  const a = await grant("a".repeat(32), "fixture-client-A", tokenA),
    b = await grant("b".repeat(32), "fixture-client-B", tokenB);
  const store = (grants) => JSON.stringify({ schema: "devbox.api-playground.mcp-oauth-grants", version: 1, grants });
  const openMcp = async () => {
    await context.ui.click(button("프로토콜"));
    await context.ui.waitForTarget({ role: "tab", name: "MCP" });
    await context.ui.click({ role: "tab", name: "MCP" });
    await context.ui.fill(textbox("MCP 엔드포인트"), fixture.endpoint);
    await context.ui.click(button("OAuth grant 새로 고침"));
  };
  const selectGrant = async (id) => {
    let index = -1;
    await until(async () => {
      index = await context.cdp.evaluate(
        `Array.from(document.querySelector('[aria-label="OAuth grant"]')?.options ?? []).findIndex(option=>option.value===${JSON.stringify(id)})`,
      );
      return index >= 0;
    }, "Requested OAuth grant option was not ready");
    await select(context, "OAuth grant", index);
  };
  const connect = async () => {
    await context.ui.click(button("연결"));
    await expectText(context, "연결에 적용된 인증:");
  };
  const invoke = async () => {
    await context.ui.click(button("목록 조회"));
    await until(
      async () =>
        await context.cdp.evaluate(
          "Boolean(Array.from(document.querySelectorAll('button')).find(item=>item.textContent.trim()==='선택 tool 호출'&&!item.disabled))",
        ),
      "Tool list did not become invokable",
    );
    const count = fixture.calls.filter((item) => item.method === "tools/call").length;
    await context.ui.click(button("선택 tool 호출"));
    if (await context.cdp.evaluate("document.querySelector('[aria-label=\"MCP 전송 방식\"]').value==='http'")) {
      await until(
        () => fixture.calls.filter((item) => item.method === "tools/call").length === count + 1,
        "Native tool call not received",
      );
    }
    await expectText(context, "synthetic tool result");
  };
  try {
    results.push(
      await scenario(context, "AUTH-01", async (record) => {
        await writeFile(file, store([a, b]));
        record(
          "L4 fixture preparation creates two bounded synthetic OAuth grants using native DPAPI ciphertext in only the prepared installed namespace",
        );
        await openMcp();
        await selectGrant(a.grantId);
        await connect();
        assert.equal(
          await context.cdp.evaluate("document.querySelector('[aria-label=\"OAuth grant\"]').disabled"),
          true,
        );
        await expectText(context, "연결에 적용된 인증: fixture-client-A");
        await invoke();
        assert.equal(
          fixture.calls.filter((item) => item.method === "tools/call").at(-1).authorization,
          `Bearer ${tokenA}`,
        );
        assert.ok(!(await bodyText(context)).includes(tokenA));
        assert.ok(!(await bodyText(context)).includes(tokenB));
        record(
          "Actual A connection locks grant selection; native tools/call retains Authorization A while displayed connection metadata identifies A and hides both tokens",
        );
        const count = fixture.calls.length;
        await context.ui.click(button("OAuth grant 취소"));
        await expectText(context, "권한 확인 필요");
        assert.equal(fixture.revocations.at(-1).token, tokenA);
        assert.ok(!JSON.parse(await readFile(file, "utf8")).grants.some((item) => item.grantId === a.grantId));
        await context.ui.click(button("OAuth grant 새로 고침"));
        await expectText(context, "권한 확인 필요");
        assert.notEqual(
          await context.cdp.evaluate("document.querySelector('[aria-label=\"OAuth grant\"]').value"),
          b.grantId,
        );
        await expectText(context, "연결에 적용된 인증: fixture-client-A");
        assert.equal(
          await context.cdp.evaluate(
            "Array.from(document.querySelectorAll('button')).find(item=>item.textContent.trim()==='선택 tool 호출').disabled",
          ),
          true,
        );
        assert.equal(fixture.calls.length, count);
        record(
          "Actual connected A revocation and grant refresh retain invalidated A connection metadata without silently selecting B or invoking another tool",
        );
        await context.ui.click(button("연결 해제"));
        await reseedOAuthGrantFixture(context, file, store([a, b]));
        record(
          "L4 reseeds synthetic grants only for a restarted native owner before the independent revocation journey",
        );
        await openMcp();
      }),
    );
    if (results.at(-1).status !== "PASS") return results;
    results.push(
      await scenario(context, "AUTH-02", async (record) => {
        await selectGrant(a.grantId);
        await connect();
        await invoke();
        await context.ui.click(button("OAuth grant 취소"));
        await expectText(context, "권한 확인 필요");
        assert.equal(fixture.revocations.at(-1).token, tokenA);
        const persisted = JSON.parse(await readFile(file, "utf8"));
        assert.ok(!persisted.grants.some((item) => item.grantId === a.grantId));
        record(
          "Actual UI revoke sends synthetic token A to owned revocation endpoint and removes only A from native grant storage",
        );
        await context.ui.click(button("연결 해제"));
        await context.ui.click(button("OAuth grant 새로 고침"));
        await selectGrant(b.grantId);
        await connect();
        await invoke();
        assert.equal(
          fixture.calls.filter((item) => item.method === "tools/call").at(-1).authorization,
          `Bearer ${tokenB}`,
        );
        await expectText(context, "연결에 적용된 인증: fixture-client-B");
        await context.ui.click(button("연결 해제"));
        record(
          "Actual explicit disconnect/select B/reconnect switches native tool Authorization to B and updates the applied authentication projection",
        );
        const executable = path.join(context.fixtureRoot, `node-mcp-${randomUUID()}.exe`),
          script = path.join(context.fixtureRoot, `stdio-mcp-${randomUUID()}.mjs`),
          pidFile = path.join(context.fixtureRoot, `stdio-pid-${randomUUID()}.json`);
        await copyFile(process.execPath, executable);
        await writeFile(
          script,
          `import{createInterface}from'node:readline';import{writeFileSync}from'node:fs';writeFileSync(${JSON.stringify(pidFile)},JSON.stringify({pid:process.pid}));const discover=${discover.toString()};const toolReply=${toolReply.toString()};createInterface({input:process.stdin}).on('line',line=>{const message=JSON.parse(line);if(message.id)process.stdout.write(JSON.stringify(toolReply(message))+'\\n');});`,
          { flag: "wx" },
        );
        await select(context, "MCP 전송 방식", 1);
        await context.ui.click(button("실행 파일 선택"));
        context.chooseFile(executable);
        await context.ui.click(button("인자 추가"));
        await context.ui.fill(textbox("stdio 인자 1"), script);
        await context.ui.click(button("연결"));
        await until(
          async () => await context.cdp.evaluate("Boolean(document.querySelector('.mcp-server-card'))"),
          "Stdio did not connect",
        );
        await invoke();
        const pid = JSON.parse(await readFile(pidFile, "utf8")).pid;
        assert.ok(
          allWindowsProcesses().some(
            (item) => item.Pid === pid && item.Path?.toLowerCase() === executable.toLowerCase(),
          ),
        );
        await context.ui.click(button("연결 해제"));
        await until(
          () => !allWindowsProcesses().some((item) => item.Pid === pid),
          "Owned stdio child survived UI disconnect",
        );
        record(
          "Actual native executable picker and visible stdio arguments connect/invoke/disconnect a disposable MCP child; observed owned child PID exits",
        );
      }),
    );
    return results;
  } finally {
    await fixture.close();
  }
}
