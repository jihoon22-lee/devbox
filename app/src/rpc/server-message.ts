import { RPC_ERROR_CODES, type RpcErrorCode } from "./gen/rpc";
export type ServerMessage =
  | { type: "welcome"; version: string; protocol: number; daemonId: string }
  | {
      type: "response";
      id: number;
      result?: unknown;
      error?: { code: RpcErrorCode; detail?: unknown };
      diagnosticId?: string;
    }
  | { type: "subscribed"; id: number; subId: number; rev: string }
  | { type: "event"; subId: number; rev: string; payload: unknown }
  | { type: "resync"; subId: number };
const object = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const uint = (v: unknown) => typeof v === "number" && Number.isInteger(v) && v >= 0 && v <= 0xffffffff;
const codes = new Set<string>(RPC_ERROR_CODES);
export function parseServerMessage(body: Uint8Array): ServerMessage {
  const msg: unknown = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(body));
  if (!object(msg)) throw new Error("Invalid server envelope");
  let valid = false;
  switch (msg.type) {
    case "welcome":
      valid = typeof msg.version === "string" && uint(msg.protocol) && typeof msg.daemonId === "string";
      break;
    case "response": {
      const error = msg.error;
      valid =
        uint(msg.id) &&
        (msg.diagnosticId === undefined || typeof msg.diagnosticId === "string") &&
        (Object.hasOwn(msg, "result")
          ? error === undefined
          : object(error) && typeof error.code === "string" && codes.has(error.code));
      break;
    }
    case "subscribed":
      valid = uint(msg.id) && uint(msg.subId) && typeof msg.rev === "string" && /^[0-9]+$/.test(msg.rev);
      break;
    case "event":
      valid =
        uint(msg.subId) && typeof msg.rev === "string" && /^[0-9]+$/.test(msg.rev) && Object.hasOwn(msg, "payload");
      break;
    case "resync":
      valid = uint(msg.subId);
      break;
  }
  if (!valid) throw new Error("Invalid server envelope");
  return msg as ServerMessage;
}
