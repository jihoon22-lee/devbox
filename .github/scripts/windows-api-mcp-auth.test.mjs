import * as runner from "./windows-api-mcp-auth.mjs";
import { scenarioModuleContract } from "./windows-api-user-flow-contract.test-support.mjs";
scenarioModuleContract(runner, ["AUTH-01", "AUTH-02"], "windows-api-mcp-auth.mjs");
