// The fixture selects the actual native command. The component is not sent in
// the typed wire envelope; every other field is retained for rejection probes.
export function typedComponentInvoke(invoke, product, payload) {
  const { component, ...request } = payload.request;
  const commands = {
    "knowledge.activity": "plugin:knowledge|activity",
    "knowledge.notes": "plugin:knowledge|notes",
    "knowledge.search": "plugin:knowledge|search",
    "knowledge.search-settings": "plugin:knowledge|search_settings",
    "knowledge.opener": "plugin:knowledge|opener",
    "knowledge.setup": "plugin:knowledge|setup",
    "knowledge.commands": "plugin:knowledge|commands",
    "api-studio.api": "plugin:api-studio|api",
    "api-studio.webhooks": "plugin:api-studio|webhooks",
    "api-studio.transforms": "plugin:api-studio|transforms",
  };
  return invoke(commands[component] ?? `plugin:${product}|invalid_component`, { ...payload, request });
}
export const typedComponentBridge = `const invokeComponent = (product, payload) => (${typedComponentInvoke.toString()})(invoke, product, payload);`;
