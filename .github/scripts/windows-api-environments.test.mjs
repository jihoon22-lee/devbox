import * as runner from "./windows-api-environments.mjs";
import { scenarioModuleContract } from "./windows-api-user-flow-contract.test-support.mjs";
scenarioModuleContract(runner,["ENV-01", "ENV-02"],"windows-api-environments.mjs");
