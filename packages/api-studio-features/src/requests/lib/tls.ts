import type { RequestTls } from "../../generated/RequestTls";
export function normalizeRequestTls(value: unknown): RequestTls | undefined {
  if (!value || typeof value !== "object") return undefined;
  const tls = value as Partial<RequestTls>;
  return {
    credentialId:
      typeof tls.credentialId === "string" && /^[a-f0-9]{32}$/.test(tls.credentialId) ? tls.credentialId : null,
    verify: tls.verify !== false,
  };
}

export function hasCustomTls(tls: RequestTls | null | undefined): boolean {
  return Boolean(tls?.credentialId) || tls?.verify === false;
}
