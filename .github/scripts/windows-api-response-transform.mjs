// Observe only fixed UI state. Never export selected response text or credentials.
export const responseTransformObservation = `(() => {
  const body = document.querySelector('.api-feature-requests .resp-body');
  const selection = getSelection();
  const range = selection?.rangeCount === 1 ? selection.getRangeAt(0) : null;
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
