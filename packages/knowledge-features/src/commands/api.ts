import { typedCall } from "../typed";
import type { QuitCall } from "../generated/QuitCall";
import type { QuitResults } from "../generated/quit-results";
export const quitCall = typedCall<QuitCall, QuitResults>("knowledge.commands");
