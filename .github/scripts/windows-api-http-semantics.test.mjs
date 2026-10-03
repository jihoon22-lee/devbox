import * as runner from "./windows-api-http-semantics.mjs";
import { scenarioModuleContract } from "./windows-api-user-flow-contract.test-support.mjs";
scenarioModuleContract(runner,["HTTP-01", "HTTP-02", "HTTP-03"],"windows-api-http-semantics.mjs");
