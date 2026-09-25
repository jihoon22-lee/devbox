import { typedCall } from "./typed";
import type { ControlToolsCall } from "./generated/ControlToolsCall";
import type { ToolsResults } from "./generated/tools-results";
export const toolsCall = typedCall<ControlToolsCall, ToolsResults>("control-center.tools");
