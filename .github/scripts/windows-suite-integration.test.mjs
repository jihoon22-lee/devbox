import * as runner from "./windows-suite-integration.mjs";
import { scenarioModuleContract } from "./windows-api-user-flow-contract.test-support.mjs";
scenarioModuleContract(runner, ["HANDOFF-01", "HANDOFF-02"], "windows-suite-integration.mjs");
