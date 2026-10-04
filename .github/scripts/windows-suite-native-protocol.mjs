// Only the pinned published v0.8.1 fixture predates the delivery command split.
export function installedFixtureCommand(command, source) {
  return command === "plugin:control-center|delivery" && source === "1c97b41ee10ca0df7c062338bfe85659af025a89"
    ? "plugin:control-center|execute"
    : command;
}
export async function prepareHistoricalNativeStore(source, product, nativeStoreReady, ui) {
  if (source !== "1c97b41ee10ca0df7c062338bfe85659af025a89" || nativeStoreReady === true) return null;
  const name = { workspace: "빈 Workspace 시작", knowledge: "새 저장소로 시작" }[product];
  if (!name) return null;
  const target = { role: "button", name };
  await ui.waitForTarget(target);
  await ui.click(target);
  // A preparation receipt records only the input. Existing native health
  // assertions below must independently establish readiness and activation.
  return {
    product,
    sourceSha: source,
    evidenceKind: "historical-fixture-preparation",
    promotionEvidence: false,
    setupControl: name,
  };
}
