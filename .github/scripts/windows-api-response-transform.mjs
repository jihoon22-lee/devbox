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

// This L4 fixture uses real pointer input to release input focus, followed by
// a synthetic DOM Range. It does not claim to exercise a user drag selection.
export async function prepareResponseSelection(cdp) {
  const point = await cdp.evaluate(`(() => {
    const body = document.querySelector('.api-feature-requests .resp-body');
    if (!body) throw new Error('response-selection-body-missing');
    body.scrollIntoView({ block: 'center', inline: 'nearest' });
    const rect = body.getBoundingClientRect();
    const x = Math.max(0, rect.left) + (Math.min(innerWidth, rect.right) - Math.max(0, rect.left)) / 2;
    const y = Math.max(0, rect.top) + (Math.min(innerHeight, rect.bottom) - Math.max(0, rect.top)) / 2;
    const hit = document.elementFromPoint(x, y);
    if (!rect.width || !rect.height || !hit || !body.contains(hit)) throw new Error('response-selection-body-not-hittable');
    return { x, y };
  })()`);
  if (!Number.isFinite(point.x) || !Number.isFinite(point.y)) throw new Error("response-selection-pointer-invalid");
  await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", ...point, button: "left", clickCount: 1 });
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", ...point, button: "left", clickCount: 1 });
  await cdp.evaluate(establishResponseSelection);
}
