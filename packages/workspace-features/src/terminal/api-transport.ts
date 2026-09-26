import type { Channel } from "@tauri-apps/api/core";
import { componentInvoke } from "../transport";
import { typedCall } from "../typed";
import type { CompanionHost } from "../generated/CompanionHost";
import type { CompanionCall } from "../generated/CompanionCall";
import type { CompanionResults } from "../generated/companion-results";
import type { TerminalStreamCall } from "../generated/TerminalStreamCall";
import type { Subscription } from "../generated/Subscription";
import type { OutputBatch } from "./lib/terminalReplay";
export const terminalCall = typedCall<
  CompanionHost | Exclude<CompanionCall, { method: CompanionHost["method"] }>,
  CompanionResults
>("workspace.terminal");
export function subscribeOutput(
  sessionId: string,
  after: number,
  channel: Channel<OutputBatch>,
): Promise<Subscription> {
  const call: TerminalStreamCall = { method: "subscribe", args: { sessionId, after } };
  return componentInvoke("workspace.terminal")<Subscription>("terminal_output_stream", { ...call.args, channel });
}
