import { createContext, useContext } from "react";
import manifest from "../../../../../apps/api-studio-tools.json";
import type { PipelineStep, PipelineValueType } from "../workflows/transformPipeline";

export type OutputSource =
  | { kind: "tool"; toolId: string }
  | { kind: "pipeline"; inputType: PipelineValueType; steps: readonly PipelineStep[] };
export const OutputSourceContext = createContext<OutputSource | undefined>(undefined);
export const useOutputSource = () => useContext(OutputSourceContext);
export const TOOL_COMMANDS = manifest.tools;
export function mayExport(source: OutputSource | undefined): boolean {
  if (!source) return false;
  if (source.kind === "pipeline") return source.steps.length > 0 && source.steps.length <= 8;
  const tool = TOOL_COMMANDS.find((tool) => tool.id === source.toolId);
  return !!tool && tool.policy !== "non-persistable";
}

/** Copy immutable UI steps into the generated native input shape. */
export function nativeOutputSource(source: OutputSource): import("../../generated/OutputSource").OutputSource {
  return source.kind === "pipeline" ? { ...source, steps: source.steps.map((step) => ({ ...step })) } : source;
}
