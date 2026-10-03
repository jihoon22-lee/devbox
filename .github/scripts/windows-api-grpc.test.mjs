import * as runner from "./windows-api-grpc.mjs";
import { scenarioModuleContract } from "./windows-api-user-flow-contract.test-support.mjs";
scenarioModuleContract(runner, ["GRPC-01"], "windows-api-grpc.mjs");
