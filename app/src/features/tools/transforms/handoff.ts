import { createContext, useContext } from "react";
import { create } from "zustand";
import { useToasts } from "../../../ui";
import type { ToolsSearch } from "../search";
import type { ToolId } from "./catalog";
import { detectSmartInput, type SmartCandidate } from "./detect";

export const MAX_HANDOFF_BYTES = 512 * 1024;
export type HandoffSource = "clipboard" | "log" | "terminal";
export interface TransformHandoff {
  id: number;
  text: string;
  source: HandoffSource;
  toolId: ToolId;
  candidate?: SmartCandidate;
}
export const useTransformHandoff = create<{ pending: TransformHandoff | null }>(() => ({ pending: null }));
export const TransformInputContext = createContext<TransformHandoff | null>(null);
export const useTransformInput = () => useContext(TransformInputContext);
type Navigate = (search: ToolsSearch) => void | Promise<unknown>;
let navigation: Navigate | undefined;
let serial = 0;
let intentRevision = 0;
/** A newer clipboard invocation or direct selection supersedes pending reads. */
export function claimTransformIntent(): () => boolean {
  const revision = ++intentRevision;
  return () => revision === intentRevision;
}
export function registerTransformsNavigation(navigate: Navigate) {
  navigation = navigate;
  return () => {
    if (navigation !== navigate) return;
    navigation = undefined;
    intentRevision++;
    useTransformHandoff.setState({ pending: null });
  };
}

function boundedText(text: string): string {
  // encodeInto stops on a complete Unicode scalar and allocates only the cap.
  // Slice the original UTF-16 text so even malformed Unicode is not rewritten.
  const { read } = new TextEncoder().encodeInto(text, new Uint8Array(MAX_HANDOFF_BYTES));
  return text.slice(0, read);
}

export async function sendToTransforms(text: string, source: HandoffSource): Promise<void> {
  if (!text) return;
  const ownsIntent = claimTransformIntent();
  const navigate = navigation;
  if (!navigate) {
    useToasts.getState().push({ tone: "warn", title: "변환 도구를 열 수 없습니다." });
    return;
  }
  const bounded = boundedText(text);
  const candidate = detectSmartInput(text).candidates[0];
  const transfer: TransformHandoff = {
    id: ++serial,
    text: bounded,
    source,
    toolId: candidate?.toolId ?? "json-format",
    candidate,
  };
  useTransformHandoff.setState({ pending: transfer });
  if (bounded !== text)
    useToasts.getState().push({ tone: "warn", title: "입력이 512KiB를 초과하여 앞부분만 보냈습니다." });
  try {
    await navigate({ view: "transforms", tool: transfer.toolId });
  } catch {
    if (!ownsIntent()) return;
    if (useTransformHandoff.getState().pending?.id === transfer.id) useTransformHandoff.setState({ pending: null });
    useToasts.getState().push({ tone: "warn", title: "변환 도구를 열지 못했습니다." });
  }
}
