import { expect, it } from "vitest";
import { safeGrpcErrorCode } from "../../../packages/api-studio-features/src/requests/grpcApi";
import { componentFailure } from "./componentErrors";
it("preserves terminal protocol errors without reflecting untrusted details", () => {
  expect(safeGrpcErrorCode(componentFailure("api-studio.api", { issue: "grpc_connection_stale" }))).toBe(
    "grpc_connection_stale",
  );
  for (const value of [
    { issue: "grpc_connection_stale synthetic-secret" },
    { issue: "grpc_connection_stale", extra: "synthetic-secret" },
    ["grpc_connection_stale"],
  ]) {
    expect(componentFailure("api-studio.api", value).message).toBe("작업을 완료하지 못했습니다.");
  }
  expect(componentFailure("api-studio.transforms", { issue: "mcp_request_cancelled" }).message).toBe(
    "작업을 완료하지 못했습니다.",
  );
});

it("preserves the reviewed Webhook receiver-unavailable message only for its owner", () => {
  const message = "Workspace Logs에 연결하지 못했습니다. 원본 요청과 fixture는 유지됩니다.";
  const issue = "native_error_9b26c2d9449d";
  expect(componentFailure("api-studio.webhooks", { issue }).message).toBe(message);
  expect(componentFailure("api-studio.api", { issue }).message).toBe("작업을 완료하지 못했습니다.");
  expect(componentFailure("api-studio.webhooks", { issue: issue + " synthetic-secret" }).message).toBe(
    "작업을 완료하지 못했습니다.",
  );
});

it("preserves the binary webhook handoff explanation", () => {
  const message = "바이너리 본문 fixture는 API 요청으로 보낼 수 없습니다";
  const issue = "native_error_974bbe3cd37f";
  expect(componentFailure("api-studio.webhooks", { issue }).message).toBe(message);
  expect(componentFailure("api-studio.api", { issue }).message).toBe("작업을 완료하지 못했습니다.");
});

it("projects document conflicts by their exact code without exposing stored data", () => {
  const conflict = componentFailure("api-studio.store", { issue: "store_revision_conflict" });
  expect(conflict.name).toBe("store_revision_conflict");
  expect(conflict.message).toBe("다른 곳에서 바뀌었습니다. 다시 불러온 뒤 저장해 주세요.");
  expect(componentFailure("api-studio.store", { issue: "store_unavailable synthetic-secret" }).message).toBe(
    "작업을 완료하지 못했습니다.",
  );
});
