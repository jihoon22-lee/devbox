import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, isTauriMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  isTauriMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("./lib/isTauri", () => ({ isTauri: isTauriMock }));

import {
  cancelGrpc,
  connectGrpc,
  disconnectGrpc,
  exportGrpcSummary,
  importGrpcTlsCredential,
  invokeGrpc,
  listGrpcTlsCredentials,
  pickGrpcProto,
  type GrpcConnectProfile,
  type GrpcConnectResult,
  type GrpcExchangeSummary,
} from "./grpcApi";

const CONNECTION_ID = "a".repeat(32);
const CREDENTIAL_ID = "b".repeat(32);
const SELECTION_ID = "c".repeat(32);

const reflectionProfile: GrpcConnectProfile = {
  endpoint: "http://127.0.0.1:50051",
  source: { kind: "reflection" },
  tls: { rootMode: "native" },
  connectTimeoutMs: 10_000,
  rpcTimeoutMs: 30_000,
};

const summary: GrpcExchangeSummary = {
  sourceKind: "reflection-v1",
  service: "Greeter",
  method: "SayHello",
  rpcKind: "unary",
  requestMessageCount: 1,
  responseMessageCount: 1,
  startedAtMs: 1_700_000_000_000,
  elapsedMs: 4,
  status: "OK",
  tlsMode: "plaintext",
  credentialUsed: false,
};

function validConnection(overrides: Partial<GrpcConnectResult> = {}): GrpcConnectResult {
  return {
    connectionId: CONNECTION_ID,
    authority: "127.0.0.1:50051",
    source: {
      kind: "reflection-v1",
      label: null,
      descriptorFileCount: 1,
      serviceCount: 1,
    },
    tls: {
      mode: "plaintext",
      encrypted: false,
      credentialUsed: false,
      serverNameOverridden: false,
    },
    methods: [{
      service: "Greeter",
      method: "SayHello",
      fullName: "Greeter.SayHello",
      inputType: "HelloRequest",
      outputType: "HelloReply",
      rpcKind: "unary",
      inputTemplate: { name: "fixture" },
    }],
    rpcTimeoutMs: 30_000,
    ...overrides,
  };
}

beforeEach(() => {
  invokeMock.mockReset();
  isTauriMock.mockReset().mockReturnValue(true);
});

describe("gRPC native IPC boundary", () => {
  it("requires the native app for every native-only operation", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(pickGrpcProto()).rejects.toThrow("grpc_native_required");
    await expect(listGrpcTlsCredentials()).rejects.toThrow("grpc_native_required");
    await expect(connectGrpc(reflectionProfile)).rejects.toThrow("grpc_native_required");
    await expect(invokeGrpc(CONNECTION_ID, "request-1", "Greeter.SayHello", ["{}"]))
      .rejects.toThrow("grpc_native_required");
    await expect(cancelGrpc(CONNECTION_ID, "request-1")).rejects.toThrow("grpc_native_required");
    await expect(disconnectGrpc(CONNECTION_ID)).rejects.toThrow("grpc_native_required");
    await expect(exportGrpcSummary(summary)).rejects.toThrow("grpc_native_required");

    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("projects picker results to opaque labels and never exposes the native path", async () => {
    invokeMock.mockResolvedValueOnce({
      selectionId: SELECTION_ID,
      kind: "proto",
      label: "fixture.proto",
      expiresAtMs: 1_700_000_000_000,
      path: "C:\\Users\\secret\\fixture.proto",
    });

    await expect(pickGrpcProto()).resolves.toEqual({
      selectionId: SELECTION_ID,
      kind: "proto",
      label: "fixture.proto",
      expiresAtMs: 1_700_000_000_000,
    });
    expect(invokeMock).toHaveBeenCalledWith("pick_grpc_proto");

    invokeMock.mockResolvedValueOnce({
      selectionId: SELECTION_ID,
      kind: "proto",
      label: "C:\\Users\\secret\\fixture.proto",
      expiresAtMs: 1_700_000_000_000,
    });
    await expect(pickGrpcProto()).rejects.toThrow("grpc_source_selection_invalid");
  });

  it("strips raw credential material from validated projections", async () => {
    invokeMock.mockResolvedValueOnce({
      credentialId: CREDENTIAL_ID,
      label: "fixture mTLS",
      hasCustomCa: true,
      hasClientIdentity: true,
      createdAtMs: 1_700_000_000_000,
      path: "C:\\Users\\secret\\client-key.pem",
      caPem: "-----BEGIN CERTIFICATE-----SECRET",
      privateKeyPem: "-----BEGIN PRIVATE KEY-----SECRET",
    });

    await expect(importGrpcTlsCredential({
      label: "fixture mTLS",
      caSelectionId: SELECTION_ID,
      clientCertificateSelectionId: SELECTION_ID,
      clientKeySelectionId: SELECTION_ID,
    })).resolves.toEqual({
      credentialId: CREDENTIAL_ID,
      label: "fixture mTLS",
      hasCustomCa: true,
      hasClientIdentity: true,
      createdAtMs: 1_700_000_000_000,
    });
    expect(invokeMock).toHaveBeenCalledWith("import_grpc_tls_credential", {
      label: "fixture mTLS",
      caSelectionId: SELECTION_ID,
      clientCertificateSelectionId: SELECTION_ID,
      clientKeySelectionId: SELECTION_ID,
    });

    invokeMock.mockResolvedValueOnce([{
      credentialId: CREDENTIAL_ID,
      label: "fixture mTLS",
      hasCustomCa: true,
      hasClientIdentity: true,
      createdAtMs: 1_700_000_000_000,
      path: "C:\\Users\\secret\\client-key.pem",
      pem: "-----BEGIN PRIVATE KEY-----SECRET",
    }]);
    await expect(listGrpcTlsCredentials()).resolves.toEqual([{
      credentialId: CREDENTIAL_ID,
      label: "fixture mTLS",
      hasCustomCa: true,
      hasClientIdentity: true,
      createdAtMs: 1_700_000_000_000,
    }]);
  });

  it("cleans up a connection when the native projection is malformed", async () => {
    invokeMock
      .mockResolvedValueOnce(validConnection({ methods: [] }))
      .mockResolvedValueOnce(undefined);

    await expect(connectGrpc(reflectionProfile)).rejects.toThrow("grpc_protocol_failed");
    expect(invokeMock).toHaveBeenNthCalledWith(1, "connect_grpc", { profile: reflectionProfile });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "disconnect_grpc", {
      connectionId: CONNECTION_ID,
    });
  });

  it("validates method projections before returning the connection", async () => {
    invokeMock.mockResolvedValueOnce({
      ...validConnection(),
      methods: [{
        ...validConnection().methods[0],
        inputTemplate: { nested: ["safe"] },
        path: "C:\\Users\\secret\\fixture.proto",
      }],
    });

    await expect(connectGrpc(reflectionProfile)).resolves.toEqual({
      ...validConnection(),
      methods: [{
        ...validConnection().methods[0],
        inputTemplate: { nested: ["safe"] },
      }],
    });
  });

  it("rejects inconsistent source, TLS, and descriptor projections", async () => {
    invokeMock
      .mockResolvedValueOnce(validConnection({
        tls: {
          mode: "plaintext",
          encrypted: true,
          credentialUsed: false,
          serverNameOverridden: false,
        },
      }))
      .mockResolvedValueOnce(undefined);
    await expect(connectGrpc(reflectionProfile)).rejects.toThrow("grpc_protocol_failed");

    invokeMock
      .mockResolvedValueOnce(validConnection({
        source: {
          kind: "reflection-v1",
          label: "secret.proto",
          descriptorFileCount: 1,
          serviceCount: 1,
        },
      }))
      .mockResolvedValueOnce(undefined);
    await expect(connectGrpc(reflectionProfile)).rejects.toThrow("grpc_protocol_failed");

    invokeMock
      .mockResolvedValueOnce(validConnection({
        methods: [{
          ...validConnection().methods[0],
          fullName: "Other.SayHello",
        }],
      }))
      .mockResolvedValueOnce(undefined);
    await expect(connectGrpc(reflectionProfile)).rejects.toThrow("grpc_protocol_failed");
  });

  it("rejects invalid request identifiers and enforces message count and byte bounds", async () => {
    await expect(invokeGrpc("z".repeat(32), "request-1", "Greeter.SayHello", ["{}"]))
      .rejects.toThrow("grpc_request_invalid");
    await expect(invokeGrpc(CONNECTION_ID, "request with spaces", "Greeter.SayHello", ["{}"]))
      .rejects.toThrow("grpc_request_invalid");
    await expect(invokeGrpc(CONNECTION_ID, "request-1", "Greeter SayHello", ["{}"]))
      .rejects.toThrow("grpc_request_invalid");
    await expect(invokeGrpc(
      CONNECTION_ID,
      "request-1",
      "Greeter.SayHello",
      Array.from({ length: 101 }, () => "{}"),
    )).rejects.toThrow("grpc_request_invalid");

    const oversizedMessage = JSON.stringify("x".repeat(1024 * 1024));
    await expect(invokeGrpc(CONNECTION_ID, "request-1", "Greeter.SayHello", [oversizedMessage]))
      .rejects.toThrow("grpc_request_too_large");

    const chunk = JSON.stringify("x".repeat(900_000));
    await expect(invokeGrpc(
      CONNECTION_ID,
      "request-1",
      "Greeter.SayHello",
      [chunk, chunk, chunk, chunk, chunk],
    )).rejects.toThrow("grpc_request_too_large");

    await expect(invokeGrpc(CONNECTION_ID, "request-1", "Greeter.SayHello", ["not-json"]))
      .rejects.toThrow("grpc_request_invalid");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("rejects out-of-bounds summary exports before crossing IPC", async () => {
    await expect(exportGrpcSummary({
      ...summary,
      requestMessageCount: 2,
    })).rejects.toThrow("grpc_export_failed");
    await expect(exportGrpcSummary({
      ...summary,
      tlsMode: "plaintext",
      credentialUsed: true,
    })).rejects.toThrow("grpc_export_failed");
    await expect(exportGrpcSummary({
      ...summary,
      startedAtMs: 8_640_000_000_000_001,
    })).rejects.toThrow("grpc_export_failed");
    expect(invokeMock).not.toHaveBeenCalled();
  });
});
