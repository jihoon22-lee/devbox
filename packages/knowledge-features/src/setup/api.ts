import { typedCall } from "../typed";
import type { SetupCall } from "../generated/SetupCall";
import type { SetupResults } from "../generated/setup-results";
export const setupCall = typedCall<SetupCall, SetupResults>("knowledge.setup");
