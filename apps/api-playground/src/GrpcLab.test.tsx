import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { GRPC_HISTORY_KEY, GRPC_HISTORY_SCHEMA } from "./lib/grpc";
import { GrpcLab } from "./GrpcLab";
import type {
  GrpcConnectResult,
  GrpcCredentialProjection,
  GrpcInvokeResult,
  GrpcNativeSelection,
} from "./grpcApi";

const mocks = vi.hoisted(() => ({
  cancel: vi.fn(),
  connect: vi.fn(),
  deleteCredential: vi.fn(),
  disconnect: vi.fn(),
  exportSummary: vi.fn(),
  importCredential: vi.fn(),
  invoke: vi.fn(),
  listCredentials: vi.fn(),
  nextRequestId: vi.fn(),
  pickCa: vi.fn(),
  pickCertificate: vi.fn(),
  pickClientKey: vi.fn(),
  pickImportRoot: vi.fn(),
  pickProto: vi.fn(),
  safeErrorCode: vi.fn(),
}));

vi.mock("./grpcApi", () => ({
  cancelGrpc: mocks.cancel,
  connectGrpc: mocks.connect,
  deleteGrpcTlsCredential: mocks.deleteCredential,
  disconnectGrpc: mocks.disconnect,
  exportGrpcSummary: mocks.exportSummary,
  importGrpcTlsCredential: mocks.importCredential,
  invokeGrpc: mocks.invoke,
  listGrpcTlsCredentials: mocks.listCredentials,
  nextGrpcRequestId: mocks.nextRequestId,
  pickGrpcCa: mocks.pickCa,
  pickGrpcClientCertificate: mocks.pickCertificate,
  pickGrpcClientKey: mocks.pickClientKey,
  pickGrpcImportRoot: mocks.pickImportRoot,
  pickGrpcProto: mocks.pickProto,
  safeGrpcErrorCode: mocks.safeErrorCode,
}));

const protoSelection: GrpcNativeSelection = {
  selectionId: "a".repeat(32),
  kind: "proto",
  label: "fixture.proto",
  expiresAtMs: 1_700_000_060_000,
};

const importRootSelection: GrpcNativeSelection = {
  selectionId: "b".repeat(32),
  kind: "import-root",
  label: "fixture imports",
  expiresAtMs: 1_700_000_060_000,
};

const credential: GrpcCredentialProjection = {
  credentialId: "c".repeat(32),
  label: "fixture mTLS",
  hasCustomCa: true,
  hasClientIdentity: true,
  createdAtMs: 1_700_000_000_000,
};

const credentialWithRawFields = {
  ...credential,
  path: "C:\\Users\\secret\\client-key.pem",
  caPem: "-----BEGIN CERTIFICATE-----SECRET",
  privateKeyPem: "-----BEGIN PRIVATE KEY-----SECRET",
};

const connection: GrpcConnectResult = {
  connectionId: "d".repeat(32),
  authority: "127.0.0.1:50051",
  source: {
    kind: "local-proto",
    label: "fixture.proto",
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
    inputTemplate: {},
  }],
  rpcTimeoutMs: 30_000,
};

const invokeResult: GrpcInvokeResult = {
  ok: true,
  status: "OK",
  responses: [{ message: "hello" }],
  requestMessageCount: 1,
  responseMessageCount: 1,
  startedAtMs: 1_700_000_000_010,
  elapsedMs: 7,
};

beforeEach(() => {
  localStorage.clear();
  mocks.cancel.mockReset().mockResolvedValue(true);
  mocks.connect.mockReset().mockResolvedValue(connection);
  mocks.deleteCredential.mockReset().mockResolvedValue(true);
  mocks.disconnect.mockReset().mockResolvedValue(undefined);
  mocks.exportSummary.mockReset().mockResolvedValue(true);
  mocks.importCredential.mockReset();
  mocks.invoke.mockReset().mockResolvedValue(invokeResult);
  mocks.listCredentials.mockReset().mockResolvedValue([credentialWithRawFields]);
  mocks.nextRequestId.mockReset().mockReturnValue("grpc-1");
  mocks.pickCa.mockReset().mockResolvedValue(null);
  mocks.pickCertificate.mockReset().mockResolvedValue(null);
  mocks.pickClientKey.mockReset().mockResolvedValue(null);
  mocks.pickImportRoot.mockReset().mockResolvedValue(importRootSelection);
  mocks.pickProto.mockReset().mockResolvedValue(protoSelection);
  mocks.safeErrorCode.mockReset().mockImplementation((cause: unknown) => (
    cause instanceof Error ? cause.message : String(cause)
  ));
});

afterEach(() => cleanup());

async function connectLocalProto(): Promise<void> {
  fireEvent.click(screen.getByRole("button", { name: "Choose proto" }));
  await screen.findByText(protoSelection.label);
  fireEvent.click(screen.getByRole("button", { name: "Choose import root" }));
  await screen.findByText(importRootSelection.label);
  await waitFor(() => expect(
    (screen.getByRole("button", { name: "Connect gRPC" }) as HTMLButtonElement).disabled,
  ).toBe(false));
  fireEvent.click(screen.getByRole("button", { name: "Connect gRPC" }));
  await screen.findByRole("heading", { name: "Method explorer" });
}

describe("gRPC Protocol Lab", () => {
  it("keeps browser preview native-only and disables all native actions", () => {
    render(<GrpcLab native={false} />);

    expect(screen.getByText(/브라우저 미리보기에서는 gRPC 연결이나 native 파일 선택/)).toBeTruthy();
    for (const name of [
      "Connect gRPC",
      "Choose proto",
      "Choose import root",
      "Choose CA",
      "Choose client certificate",
      "Choose private key",
      "Import encrypted credential",
      "Refresh credentials",
    ]) {
      expect((screen.getByRole("button", { name }) as HTMLButtonElement).disabled).toBe(true);
    }
    expect(mocks.listCredentials).not.toHaveBeenCalled();
    expect(mocks.connect).not.toHaveBeenCalled();
    expect(mocks.invoke).not.toHaveBeenCalled();
  });

  it("connects from opaque local-proto selections, invokes a method, and persists summary-only history", async () => {
    render(<GrpcLab native />);
    await screen.findByText(credential.label);
    const renderedText = document.body.textContent ?? "";
    expect(renderedText).not.toContain(credentialWithRawFields.path);
    expect(renderedText).not.toContain(credentialWithRawFields.caPem);
    expect(renderedText).not.toContain(credentialWithRawFields.privateKeyPem);

    await connectLocalProto();
    expect(mocks.connect).toHaveBeenCalledWith({
      endpoint: "http://127.0.0.1:50051",
      source: {
        kind: "local-proto",
        protoSelectionId: protoSelection.selectionId,
        importRootSelectionId: importRootSelection.selectionId,
      },
      tls: { rootMode: "native" },
      connectTimeoutMs: 10_000,
      rpcTimeoutMs: 30_000,
    });

    fireEvent.click(screen.getByRole("button", { name: "Invoke RPC" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith(
      connection.connectionId,
      "grpc-1",
      "Greeter.SayHello",
      ["{}"],
    ));
    await screen.findByText(/hello/);

    const persisted = JSON.parse(localStorage.getItem(GRPC_HISTORY_KEY) ?? "null") as {
      schema: string;
      entries: Record<string, unknown>[];
    };
    expect(persisted).toEqual({
      schema: GRPC_HISTORY_SCHEMA,
      entries: [{
        sourceKind: "local-proto",
        service: "Greeter",
        method: "SayHello",
        rpcKind: "unary",
        requestMessageCount: 1,
        responseMessageCount: 1,
        startedAtMs: invokeResult.startedAtMs,
        elapsedMs: invokeResult.elapsedMs,
        status: "OK",
        tlsMode: "plaintext",
        credentialUsed: false,
      }],
    });
    expect(JSON.stringify(persisted)).not.toContain("hello");
    expect(JSON.stringify(persisted)).not.toContain(protoSelection.selectionId);
    expect(JSON.stringify(persisted)).not.toContain(credential.credentialId);

    fireEvent.click(screen.getByRole("button", { name: "Export summary" }));
    await waitFor(() => expect(mocks.exportSummary).toHaveBeenCalledWith(persisted.entries[0]));
    await screen.findByText(/summary를 저장했습니다/);

    fireEvent.click(screen.getByRole("button", { name: "Clear history" }));
    await screen.findByText("아직 저장된 gRPC summary가 없습니다.");
    expect(JSON.parse(localStorage.getItem(GRPC_HISTORY_KEY) ?? "null")).toEqual({
      schema: GRPC_HISTORY_SCHEMA,
      entries: [],
    });

    fireEvent.click(screen.getByRole("button", { name: "연결 해제" }));
    await waitFor(() => expect(mocks.disconnect).toHaveBeenCalledWith(connection.connectionId));
  });

  it("never sends retained TLS settings to a plaintext endpoint", async () => {
    render(<GrpcLab native />);
    await screen.findByText(credential.label);

    const endpoint = screen.getByRole("textbox", { name: "gRPC endpoint" });
    fireEvent.change(endpoint, { target: { value: "https://api.example.test" } });
    fireEvent.change(screen.getByRole("combobox", { name: "gRPC TLS root mode" }), {
      target: { value: "custom" },
    });
    fireEvent.change(screen.getByRole("combobox", { name: "gRPC TLS credential" }), {
      target: { value: credential.credentialId },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "gRPC server name override" }), {
      target: { value: "mtls.example.test" },
    });
    fireEvent.change(endpoint, { target: { value: "http://127.0.0.1:50051" } });

    await connectLocalProto();
    expect(mocks.connect).toHaveBeenLastCalledWith(expect.objectContaining({
      endpoint: "http://127.0.0.1:50051",
      tls: { rootMode: "native" },
    }));
  });

  it("keeps the selected method consistent with the visible filter results", async () => {
    mocks.connect.mockResolvedValueOnce({
      ...connection,
      methods: [
        ...connection.methods,
        {
          service: "Greeter",
          method: "WatchHello",
          fullName: "Greeter.WatchHello",
          inputType: "WatchRequest",
          outputType: "HelloReply",
          rpcKind: "server-streaming",
          inputTemplate: { cursor: "" },
        },
      ],
    });
    render(<GrpcLab native />);
    await connectLocalProto();

    const filter = screen.getByRole("textbox", { name: "Filter gRPC methods" });
    const method = screen.getByRole("combobox", { name: "gRPC method" }) as HTMLSelectElement;
    fireEvent.change(filter, { target: { value: "WatchHello" } });
    await waitFor(() => expect(method.value).toBe("Greeter.WatchHello"));
    expect(screen.getByText("server-streaming")).toBeTruthy();

    fireEvent.change(filter, { target: { value: "no-match" } });
    await waitFor(() => expect(method.value).toBe(""));
    expect((screen.getByRole("button", { name: "Invoke RPC" }) as HTMLButtonElement).disabled).toBe(true);

    fireEvent.change(filter, { target: { value: "" } });
    await waitFor(() => expect(method.value).toBe("Greeter.SayHello"));
  });

  it("cancels the owned native request and records a bounded cancellation summary", async () => {
    let rejectInvoke: ((reason?: unknown) => void) | undefined;
    mocks.invoke.mockImplementationOnce(() => new Promise((_resolve, reject) => {
      rejectInvoke = reject;
    }));
    render(<GrpcLab native />);
    await connectLocalProto();

    fireEvent.click(screen.getByRole("button", { name: "Invoke RPC" }));
    await screen.findByRole("button", { name: "취소" });
    fireEvent.click(screen.getByRole("button", { name: "취소" }));
    await waitFor(() => expect(mocks.cancel).toHaveBeenCalledWith(
      connection.connectionId,
      "grpc-1",
    ));

    await act(async () => {
      rejectInvoke?.(new Error("grpc_request_cancelled"));
      await Promise.resolve();
    });
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("취소했습니다"));
    expect(JSON.parse(localStorage.getItem(GRPC_HISTORY_KEY) ?? "null")).toMatchObject({
      schema: GRPC_HISTORY_SCHEMA,
      entries: [{
        sourceKind: "local-proto",
        service: "Greeter",
        method: "SayHello",
        rpcKind: "unary",
        requestMessageCount: 1,
        responseMessageCount: 0,
        status: "CANCELLED",
        tlsMode: "plaintext",
        credentialUsed: false,
      }],
    });
  });
});
