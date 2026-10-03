import * as runner from "./windows-api-webhooks.mjs";
import { scenarioModuleContract } from "./windows-api-user-flow-contract.test-support.mjs";
scenarioModuleContract(runner,["WEB-01", "WEB-02"],"windows-api-webhooks.mjs");
