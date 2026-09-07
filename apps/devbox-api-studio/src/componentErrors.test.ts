import { expect, it } from "vitest";
import { safeGrpcErrorCode } from "../../../packages/api-studio-features/src/requests/grpcApi";
import { componentFailure } from "./componentErrors";
it("preserves terminal protocol errors without reflecting untrusted details", () => {
  expect(safeGrpcErrorCode(componentFailure("api-studio.api", { issue: "grpc_connection_stale" }))).toBe("grpc_connection_stale");
  for (const value of [{ issue: "grpc_connection_stale synthetic-secret" }, { issue: "grpc_connection_stale", extra: "synthetic-secret" }, ["grpc_connection_stale"]]) {
    expect(componentFailure("api-studio.api", value).message).toBe("작업을 완료하지 못했습니다.");
  }
  expect(componentFailure("api-studio.transforms", { issue: "mcp_request_cancelled" }).message).toBe("작업을 완료하지 못했습니다.");
});
