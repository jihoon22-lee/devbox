import type { OAuth2Config } from "../../generated/OAuth2Config";
export const emptyOAuth2: OAuth2Config = {
  grantType: "authorizationCode",
  authorizationUrl: "",
  tokenUrl: "",
  clientId: "",
  clientSecret: "",
  scopes: "",
};
function bounded(value: unknown, max: number): string {
  if (typeof value !== "string") return "";
  let result = "",
    bytes = 0;
  const encoder = new TextEncoder();
  for (const character of value) {
    bytes += encoder.encode(character).length;
    if (bytes > max) break;
    result += character;
  }
  return result;
}
export function normalizeOAuth2(value: unknown): OAuth2Config | undefined {
  if (!value || typeof value !== "object") return undefined;
  const config = value as Partial<OAuth2Config>;
  if (config.grantType !== "authorizationCode" && config.grantType !== "clientCredentials") return undefined;
  return {
    grantType: config.grantType,
    authorizationUrl: bounded(config.authorizationUrl, 8192),
    tokenUrl: bounded(config.tokenUrl, 8192),
    clientId: bounded(config.clientId, 8192),
    clientSecret: bounded(config.clientSecret, 65536),
    scopes: bounded(config.scopes, 32768),
  };
}
