import { createHash } from "node:crypto";
// Observe only fixed UI state. Never export selected response text or credentials.
export const responseTransformObservation = `(() => {
  const body = document.querySelector('.api-feature-requests .resp-body');
  const selection = getSelection();
  const range = selection?.rangeCount === 1 ? selection.getRangeAt(0) : null;
  const rect = body?.getBoundingClientRect();
  const style = body ? getComputedStyle(body) : null;
  const active = document.activeElement;
  const feedback = document.querySelector('.api-feature-requests .response-feedback');
  const text = feedback?.textContent || '';
  const codes = [
    ['selection-empty', '선택 영역이 비어 있습니다.'],
    ['selection-outside', '선택 영역은 현재 응답 본문 안에 있어야 합니다.'],
    ['selection-stale', '응답이 변경되어 선택 영역을 보낼 수 없습니다.'],
    ['native-send-failed', 'Developer Toolbox로 선택 영역을 전달하지 못했습니다.'],
    ['native-unavailable', 'Developer Toolbox로 보내기는 데스크톱 앱에서만 사용할 수 있습니다.'],
  ];
  return {
    bodyPresent: !!body,
    bodyTextNonempty: !!body?.textContent?.trim(),
    rangeTextNonempty: !!range?.toString().trim(),
    bodyRect: rect ? { x: rect.x, y: rect.y, width: rect.width, height: rect.height } : null,
    bodyVisible: !!(rect && rect.width > 0 && rect.height > 0 && style?.display !== 'none' && style?.visibility !== 'hidden' && !body.closest('[hidden]')),
    activeElementTag: active?.tagName || null,
    activeElementType: active?.tagName === 'INPUT' ? active.type : null,
    selectionInsideBody: !!(body && range && body.contains(range.startContainer) && body.contains(range.endContainer)),
    selectionNonempty: !!selection?.toString().trim(),
    senderAcknowledged: !!feedback?.classList.contains('success'),
    senderIssue: feedback?.classList.contains('error') ? (codes.find(([,prefix]) => text.startsWith(prefix))?.[0] || 'sender-error') : null,
    transformsVisible: !!document.querySelector('.api-feature-transforms:not([hidden])'),
    receiverPreview: !!document.querySelector('.api-feature-transforms:not([hidden]) [role=dialog]'),
    receiverAlert: !!document.querySelector('.api-feature-transforms:not([hidden]) [role=alert]'),
  };
})()`;

export const establishResponseSelection = `(() => {
  const body = document.querySelector('.api-feature-requests .resp-body');
  if (!body) throw new Error('response-selection-body-missing');
  const range = document.createRange();
  range.selectNodeContents(body);
  const selection = getSelection();
  if (!selection) throw new Error('response-selection-unavailable');
  selection.removeAllRanges();
  selection.addRange(range);
  // Synchronize the renderer's revision snapshot before the next CDP command.
  document.dispatchEvent(new Event('selectionchange'));
})()`;

export async function waitForResponseTransform(cdp, until, observe) {
  await until(async () => {
    const state = await cdp.evaluate(responseTransformObservation);
    observe(state);
    if (state.senderIssue) throw new Error(`response-transform-${state.senderIssue}`);
    if (state.receiverAlert && !state.receiverPreview) throw new Error("response-transform-receiver-alert");
    return state.senderAcknowledged && state.transformsVisible && state.receiverPreview;
  }, "response-transform-acknowledgment-or-preview-timeout");
}

export const responseGeometryObservation = `(() => {
  const body = document.querySelector('.api-feature-requests .resp-body');
  const rectangle = (node) => { const r = node.getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height }; };
  const describe = (node) => {
    const style = getComputedStyle(node);
    return { tag: node.tagName, classes: Array.from(node.classList), rect: rectangle(node), clientHeight: node.clientHeight, scrollHeight: node.scrollHeight, overflowX: style.overflowX, overflowY: style.overflowY, lineHeight: style.lineHeight, display: style.display, visibility: style.visibility };
  };
  const ancestors = [];
  for (let node = body?.parentElement; node && ancestors.length < 12; node = node.parentElement) ancestors.push(describe(node));
  const rect = body?.getBoundingClientRect();
  const x = rect ? Math.max(0, rect.left) + (Math.min(innerWidth, rect.right) - Math.max(0, rect.left)) / 2 : null;
  const y = rect ? Math.max(0, rect.top) + (Math.min(innerHeight, rect.bottom) - Math.max(0, rect.top)) / 2 : null;
  const intersectsViewport = !!(rect && Math.min(innerWidth, rect.right) > Math.max(0, rect.left) && Math.min(innerHeight, rect.bottom) > Math.max(0, rect.top));
  const hit = intersectsViewport ? document.elementFromPoint(x, y) : null;
  return { viewportWidth: innerWidth, viewportHeight: innerHeight, body: body ? describe(body) : null, ancestors, pointer: { x, y, intersectsViewport, hitsBody: !!(body && hit && body.contains(hit)), hitTag: hit?.tagName || null, hitClasses: hit ? Array.from(hit.classList) : [] } };
})()`;

// This L4 fixture uses real pointer input followed by a synthetic DOM Range.
// It does not claim to exercise a user drag selection.
export async function prepareResponseSelection(cdp, observeGeometry = () => {}) {
  await cdp.evaluate(
    `(() => { document.querySelector('.api-feature-requests .resp-body')?.scrollIntoView({ block: 'center', inline: 'nearest' }); })()`,
  );
  const geometry = await cdp.evaluate(responseGeometryObservation);
  observeGeometry(geometry);
  if (!geometry.body) throw new Error("response-selection-body-missing");
  if (!geometry.pointer.intersectsViewport || !geometry.pointer.hitsBody)
    throw new Error("response-selection-body-not-hittable");
  const { x, y } = geometry.pointer;
  if (!Number.isFinite(x) || !Number.isFinite(y)) throw new Error("response-selection-pointer-invalid");
  await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", clickCount: 1 });
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", clickCount: 1 });
  await cdp.evaluate(establishResponseSelection);
}

export function diagnosticStylesConfig(env) {
  const flag = env.DEVBOX_API_DIAGNOSTIC_STYLES;
  if (flag === undefined) return null;
  if (flag !== "1") throw new Error("api-diagnostic-styles-invalid-flag");
  if (env.GITHUB_ACTIONS !== "true" || env.RUNNER_ENVIRONMENT !== "github-hosted")
    throw new Error("api-diagnostic-styles-hosted-runner-required");
  if (env.GITHUB_EVENT_NAME !== "workflow_dispatch" || env.GITHUB_WORKFLOW !== "Product foundation acceptance")
    throw new Error("api-diagnostic-styles-diagnostic-workflow-required");
  if (
    !/^[a-f0-9]{40}$/.test(env.GITHUB_SHA ?? "") ||
    !/^[a-f0-9]{40}$/.test(env.DEVBOX_SUITE_ARTIFACT_SOURCE ?? "") ||
    !/^[1-9][0-9]{0,19}$/.test(env.DEVBOX_SUITE_ARTIFACT_RUN ?? "")
  )
    throw new Error("api-diagnostic-styles-valid-source-run-pair-required");
  return {
    runnerSourceSha: env.GITHUB_SHA,
    artifactSource: env.DEVBOX_SUITE_ARTIFACT_SOURCE,
    artifactRun: env.DEVBOX_SUITE_ARTIFACT_RUN,
    styleSourceSha: env.GITHUB_SHA,
  };
}

export function prepareDiagnosticCss(css) {
  const imports = /^@import "@devbox\/tokens\/tokens\.css";\r?\n@import "@devbox\/a11y\/styles\.css";\r?\n/.exec(css);
  if (typeof css !== "string" || !imports) throw new Error("api-diagnostic-styles-unexpected-imports");
  return css.slice(imports[0].length);
}

export async function applyDiagnosticStyles(cdp, css, observe = () => {}) {
  if (typeof css !== "string" || !css.length || css.length > 1024 * 1024)
    throw new Error("api-diagnostic-styles-invalid-css");
  let computed;
  let stylesheet;
  try {
    await cdp.send("DOM.enable");
    await cdp.send("CSS.enable");
    const { root } = await cdp.send("DOM.getDocument");
    const { nodeId } = await cdp.send("DOM.querySelector", {
      nodeId: root.nodeId,
      selector: ".api-feature-requests .response",
    });
    if (!nodeId) throw new Error("missing-response-node");
    const matched = await cdp.send("CSS.getMatchedStylesForNode", { nodeId });
    const rules = (matched.matchedCSSRules ?? []).filter(
      ({ rule, matchingSelectors }) =>
        rule.origin === "regular" &&
        rule.styleSheetId &&
        matchingSelectors.some(
          (index) => rule.selectorList.selectors[index]?.text === ".api-feature-requests .response",
        ),
    );
    const ids = [...new Set(rules.map(({ rule }) => rule.styleSheetId))];
    if (ids.length !== 1) throw new Error("authored-response-stylesheet-not-unique");
    const styleSheetId = ids[0];
    const { text } = await cdp.send("CSS.getStyleSheetText", { styleSheetId });
    if (typeof text !== "string" || text.length > 4 * 1024 * 1024) throw new Error("invalid-authored-stylesheet");
    const appended = text + "\n/* Explicit retained API diagnostic overlay. */\n" + css;
    stylesheet = {
      injection: "authored-stylesheet-append",
      authoredStyleSha256: createHash("sha256").update(text).digest("hex"),
      combinedStyleSha256: createHash("sha256").update(appended).digest("hex"),
      matchedRules: rules.map(({ rule }) => ({
        selector: rule.selectorList.text,
        origin: rule.origin,
        declarations: rule.style.cssProperties
          .filter((property) =>
            [
              "flex",
              "flex-grow",
              "flex-shrink",
              "flex-basis",
              "min-height",
              "overflow",
              "overflow-x",
              "overflow-y",
            ].includes(property.name),
          )
          .map(({ name, value, important }) => ({ name, value, important: !!important })),
      })),
    };
    observe({ stylesheet });
    await cdp.send("CSS.setStyleSheetText", { styleSheetId, text: appended });
    computed = await cdp.evaluate(`(() => {
      const response = document.querySelector('.api-feature-requests .response');
      const body = document.querySelector('.api-feature-requests .resp-body');
      const style = response ? getComputedStyle(response) : null;
      return { responsePresent: !!response, responseFlexShrink: style?.flexShrink || null, responseMinHeight: style?.minHeight || null, bodyPresent: !!body, bodyMinHeight: body ? getComputedStyle(body).minHeight : null };
    })()`);
  } catch (error) {
    const known = ["missing-response-node", "authored-response-stylesheet-not-unique", "invalid-authored-stylesheet"];
    throw new Error(
      known.includes(error.message)
        ? "api-diagnostic-styles-" + error.message
        : "api-diagnostic-styles-authored-injection-failed",
    );
  }
  computed = { ...computed, stylesheet };
  observe(computed);
  if (
    !computed.responsePresent ||
    computed.responseFlexShrink !== "0" ||
    computed.responseMinHeight !== "320px" ||
    (computed.bodyPresent && computed.bodyMinHeight !== "160px")
  )
    throw new Error("api-diagnostic-styles-computed-style-not-confirmed");
  return computed;
}
