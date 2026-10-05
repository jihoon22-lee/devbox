import * as runner from "./windows-suite-integration.mjs";
import { scenarioModuleContract } from "./windows-api-user-flow-contract.test-support.mjs";
scenarioModuleContract(runner, ["HANDOFF-01", "HANDOFF-02"], "windows-suite-integration.mjs");
import assert from "node:assert/strict";
import test from "node:test";
import { runInNewContext } from "node:vm";
test("editor context menu readiness precedes one transform selection", async () => {
  const events = [];
  let ready = false;
  await runner.selectSource({
    cdp: {
      command: async (_method, params) => {
        events.push(params.type);
      },
    },
    ui: {
      press: async () => {},
      click: async (target) => {
        if (target.role === "menuitem") {
          assert.equal(ready, true);
          events.push("select");
        }
      },
      waitForTarget: async (target) => {
        assert.equal(target.name, "선택 내용을 API Studio에서 변환");
        ready = true;
        events.push("ready");
      },
    },
  });
  assert.deepEqual(events, ["keyDown", "keyUp", "ready", "select"]);
});
test("incoming review uses accessible region name rather than absent visible heading", async () => {
  const events = [];
  await runner.review({
    cdp: {
      evaluate: async () => {
        throw new Error("aria-label is not visible body text");
      },
    },
    ui: {
      waitForTarget: async (target) => {
        assert.deepEqual(target, { role: "region", name: "다른 제품의 열기 요청" });
        events.push("ready");
      },
      click: async (target) => {
        assert.equal(target.name, "화면 열기");
        assert.equal(target.scope.name, "다른 제품의 열기 요청");
        events.push("click");
      },
    },
  });
  assert.deepEqual(events, ["ready", "click"]);
});

// Native accept acknowledgement and React's incomingText effect complete after input dispatch.
test("source apply observes the accepted Unicode input before continuing without replay", async () => {
  let clicks = 0;
  let reads = 0;
  await runner.applySourceSelection(
    {
      ui: {
        click: async (target) => {
          assert.equal(target.name, "적용");
          assert.equal(target.scope.name, "Toolbox 텍스트 미리보기");
          clicks++;
        },
      },
      cdp: {
        evaluate: async (expression) => {
          assert.ok(expression.includes('textarea[aria-label="스마트 워크플로 입력"]'));
          assert.ok(expression.includes('.value === "원본\\n"'));
          return ++reads !== 1;
        },
      },
    },
    "원본\n",
  );
  assert.equal(clicks, 1);
  assert.equal(reads, 2);
});

test("Knowledge cancel waits for delayed native discard and preview removal without replay", async () => {
  const events = [];
  let previewReady = false;
  let releasePreview;
  const preview = new Promise((resolve) => {
    releasePreview = resolve;
  });
  let release;
  const discarded = new Promise((resolve) => {
    release = resolve;
  });
  const pending = runner.cancelKnowledgePreview({
    ui: {
      waitForTarget: async (target) => {
        assert.equal(target.name, "취소");
        assert.equal(target.scope.name, "API Studio 결과 초안 미리보기");
        events.push("wait preview");
        await preview;
        previewReady = true;
      },
      click: async (target) => {
        assert.equal(previewReady, true, "Preview must arrive before cancellation input");
        assert.equal(target.name, "취소");
        assert.equal(target.scope.name, "API Studio 결과 초안 미리보기");
        events.push("cancel");
      },
    },
    cdp: {
      evaluate: async () => {
        events.push("observe");
        await discarded;
        return true;
      },
    },
  });
  pending.catch(() => {});
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(events, ["wait preview"]);
  releasePreview();
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(events, ["wait preview", "cancel", "observe"]);
  release();
  await pending;
  assert.deepEqual(events, ["wait preview", "cancel", "observe"]);
});

test("Knowledge saved file does not authorize duplicate review before preview closes", async () => {
  let closed = false;
  let observations = 0;
  await runner.awaitKnowledgePreviewClosed({
    cdp: {
      evaluate: async () => {
        assert.equal(closed, false);
        if (++observations === 1) return false;
        closed = true;
        return true;
      },
    },
  });
  assert.equal(closed, true);
  assert.equal(observations, 2);
});

test("Knowledge save waits for its delayed preview before exactly one input", async () => {
  const events = [];
  let ready = false;
  let release;
  const preview = new Promise((resolve) => {
    release = resolve;
  });
  const pending = runner.saveKnowledgePreview({
    ui: {
      waitForTarget: async (target) => {
        assert.equal(target.name, "초안 저장");
        assert.equal(target.scope.name, "API Studio 결과 초안 미리보기");
        events.push("wait preview");
        await preview;
        ready = true;
      },
      click: async () => {
        assert.equal(ready, true, "Preview must arrive before save input");
        events.push("save");
      },
    },
  });
  pending.catch(() => {});
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(events, ["wait preview"]);
  release();
  await pending;
  assert.deepEqual(events, ["wait preview", "save"]);
});

test("connection review waits for delayed enabled approval before exactly one approval", async () => {
  const events = [];
  let release;
  const approvedReady = new Promise((resolve) => {
    release = resolve;
  });
  const operation = runner.restoreSuiteConnection({
    ui: {
      click: async (target) => {
        events.push(target.name);
      },
      waitForTarget: async (target) => {
        assert.deepEqual(target, { role: "button", name: "연결 켜기" });
        events.push("observe approval");
        await approvedReady;
      },
    },
  });
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(events, ["이 설치 확인", "observe approval"]);
  release();
  await operation;
  assert.deepEqual(events, ["이 설치 확인", "observe approval", "연결 켜기"]);
});

// Native select keyboard navigation skips disabled incompatible transformers.
test("Base64 stage selection counts enabled options and confirms the exact value", async () => {
  const options = [
    { value: "json-format", disabled: true },
    { value: "json-parse", disabled: false },
    { value: "json-to-yaml", disabled: true },
    { value: "url-encode", disabled: false },
    { value: "url-decode", disabled: false },
    { value: "base64-encode", disabled: false },
    { value: "base64-decode", disabled: true },
    { value: "hex-encode", disabled: false },
  ];
  let selected = 0;
  let clicks = 0;
  let valueReads = 0;
  const enabled = options.filter((option) => !option.disabled);
  const element = {
    options,
    get value() {
      return enabled[selected].value;
    },
  };
  const { runInNewContext } = await import("node:vm");
  await runner.selectBase64Stage({
    cdp: {
      evaluate: async (expression) => {
        if (expression.startsWith("document.querySelector")) valueReads++;
        return runInNewContext(expression, { document: { querySelector: () => element } });
      },
    },
    ui: {
      click: async () => {
        clicks++;
      },
      press: async (key) => {
        if (key === "Home") selected = 0;
        if (key === "ArrowDown") selected = Math.min(selected + 1, enabled.length - 1);
      },
    },
  });
  assert.equal(element.value, "base64-encode");
  assert.equal(clicks, 1);
  assert.equal(valueReads, 1);
});

test("connection lazy mount and status readiness precede exactly one disconnect", async () => {
  const events = [];
  let ready = false;
  await runner.disconnectSuiteConnection({
    ui: {
      click: async (target) => {
        if (target.name === "자동 연결 끄기") assert.equal(ready, true);
        events.push(target.name);
      },
      waitForTarget: async (target) => {
        assert.equal(target.name, "자동 연결 끄기");
        await Promise.resolve();
        ready = true;
        events.push("connected readiness");
      },
    },
  });
  assert.deepEqual(events, ["제품 연결", "connected readiness", "자동 연결 끄기"]);
});

test("fresh pending review must mount before expiry pipeline input without action replay", async () => {
  let release;
  const mounted = new Promise((resolve) => {
    release = resolve;
  });
  const actions = [];
  const work = runner.prepareExpiryPipeline({
    ui: {
      waitForTarget: async (target) => {
        assert.deepEqual(target, { role: "region", name: "다른 제품의 열기 요청" });
        await mounted;
      },
      click: async (target) => {
        actions.push(target.name);
      },
      press: async (key) => {
        actions.push(key);
      },
    },
  });
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(actions, []);
  release();
  await work;
  assert.deepEqual(actions, ["새 파이프라인", "파이프라인 입력 형식", "Home", "Enter"]);
});

test("handoff close discards only its dirty synthetic file and does not save changed bytes", async () => {
  for (const dirty of [[], ["C:\\owned\\source.txt"]]) {
    const clicks = [];
    await runner.reviewHandoffClose(
      { cdp: { evaluate: async () => dirty }, ui: { click: async (target) => clicks.push(target) } },
      "C:\\owned\\source.txt",
    );
    assert.deepEqual(clicks, [
      {
        role: "button",
        name: dirty.length ? "파일 변경 폐기 후 종료" : "종료",
        scope: { role: "dialog", name: "Workspace 종료 검토" },
      },
    ]);
  }
  let clicked = false;
  await assert.rejects(
    runner.reviewHandoffClose(
      {
        cdp: { evaluate: async () => ["C:\\other.txt"] },
        ui: {
          click: async () => {
            clicked = true;
          },
        },
      },
      "C:\\owned\\source.txt",
    ),
    /Unowned dirty document/,
  );
  assert.equal(clicked, false);
});

const previewSelector = '.toolbox-handoff-text[aria-label="Toolbox 전달 텍스트"]';
function previewContext(readNodes) {
  return {
    cdp: {
      evaluate: async (expression) =>
        runInNewContext(expression, {
          document: {
            body: { innerText: "Toolbox 텍스트 미리보기\n원본\nWorkspace\n전달\n만료" },
            querySelectorAll: (selector) => (selector === previewSelector ? readNodes() : []),
          },
        }),
    },
  };
}
const observeOnce = async (check, message) => assert.equal(await check(), true, message);
test("first handoff preview rejects metadata-only, wrong, and ambiguous payloads", async () => {
  const original = Buffer.from("\uFEFF실패 후 보존할 초안\r\n");
  for (const nodes of [
    [],
    [{ textContent: "" }],
    [{ textContent: "원본\n" }],
    [{ textContent: "실패 후 보존할 초안\n" }, { textContent: "실패 후 보존할 초안\n" }],
  ]) {
    await assert.rejects(() =>
      runner.observeSourcePreview(
        previewContext(() => nodes),
        original,
        observeOnce,
      ),
    );
  }
});
test("first handoff preview waits for exact retained bytes with BOM and line-ending normalization", async () => {
  const expected = "  실패 후 보존할 초안\n\n";
  const original = Buffer.from("\uFEFF  실패 후 보존할 초안\r\n\r\n");
  let reads = 0;
  const result = await runner.observeSourcePreview(
    previewContext(() => {
      reads++;
      return reads === 1 ? [] : [{ textContent: expected }];
    }),
    original,
  );
  assert.equal(result, expected);
  assert.equal(reads, 2);
});
