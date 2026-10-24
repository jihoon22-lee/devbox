import { expect, it } from "vitest";
import type { GrpcCatalog } from "../../../../rpc/gen/rpc";
import {
  applyGrpc,
  autoSelect,
  draftOf,
  editDraft,
  emptyDrafts,
  filterMethods,
  newCall,
  parseGrpcFile,
  resultTitle,
} from "./grpcDrafts";

const method = (name: string, extra = {}) => ({
  name,
  fullName: `demo.Echo/${name}`,
  clientStreaming: false,
  serverStreaming: false,
  inputDefault: `{"name":"${name.toLowerCase()}"}`,
  ...extra,
});
const catalog: GrpcCatalog = {
  source: "reflection",
  services: [
    { name: "demo.Echo", methods: [method("Alpha"), method("Beta"), method("Upload", { clientStreaming: true })] },
  ],
};
const T = "http://127.0.0.1:9";

it("grpc_filter_switch_keeps_the_dirty_draft", () => {
  let drafts = emptyDrafts();
  let selected: string | null = "demo.Echo/Alpha";
  drafts = editDraft(drafts, T, "demo.Echo/Alpha", '{"name":"edited"}');
  // 필터가 B만 남기면 자동 선택은 B로 바뀐다(동작 유지).
  let visible = filterMethods(catalog, "bet");
  expect(visible.map((m) => m.fullName)).toEqual(["demo.Echo/Beta"]);
  selected = autoSelect(selected, visible);
  expect(selected).toBe("demo.Echo/Beta");
  expect(draftOf(drafts, T, method("Beta"))).toEqual({ json: '{"name":"beta"}', dirty: false });
  // 필터를 지워도 A 초안은 그대로다.
  visible = filterMethods(catalog, "");
  expect(autoSelect(selected, visible)).toBe("demo.Echo/Beta");
  expect(draftOf(drafts, T, method("Alpha"))).toEqual({ json: '{"name":"edited"}', dirty: true });
  // 일치 없는 필터는 선택을 바꾸지 않는다.
  expect(autoSelect("demo.Echo/Alpha", filterMethods(catalog, "zzz"))).toBe("demo.Echo/Alpha");
  // 대상이 다르면 다른 초안이다.
  expect(draftOf(drafts, "http://other:1", method("Alpha")).dirty).toBe(false);
});

it("client_streaming_methods_are_listed_but_marked", () => {
  const upload = filterMethods(catalog, "upload")[0]!;
  expect(upload.clientStreaming).toBe(true);
});

it("calls_keep_their_own_method_and_apply_events_in_order", () => {
  let call = newCall(1, T, "demo.Echo/Alpha", 1000);
  call = applyGrpc(call, { type: "headers", metadata: [{ name: "x", value: "1" }] });
  call = applyGrpc(call, { type: "message", json: '{"name":"a"}' });
  call = applyGrpc(call, { type: "trailers", code: 0, message: "", metadata: [] });
  expect(call.status).toBe("ok");
  expect(call.messages).toEqual(['{"name":"a"}']);
  expect(resultTitle(call, "demo.Echo/Alpha")).toBe("결과");
  expect(resultTitle(call, "demo.Echo/Beta")).toBe("demo.Echo/Alpha의 결과");
  const failed = applyGrpc(newCall(2, T, "demo.Echo/Beta", 0), {
    type: "trailers",
    code: 7,
    message: "denied",
    metadata: [],
  });
  expect(failed.status).toBe("failed");
  const broken = applyGrpc(newCall(3, T, "demo.Echo/Beta", 0), { type: "failed", error: { code: "api.network" } });
  expect(broken.failure?.code).toBe("api.network");
});

it("grpc_files_parse_and_reject_bad_shapes", () => {
  const text = JSON.stringify({
    version: 1,
    kind: "grpc",
    name: "svc",
    grpc: {
      target: "http://127.0.0.1:9",
      metadata: [{ name: "x-a", value: "1", enabled: true }],
      tls: { verify: true, caPath: null, clientCertPath: null, clientKeyPath: null },
      protoFiles: ["/p/echo.proto"],
      method: "demo.Echo/Alpha",
      json: "{}",
    },
  });
  const parsed = parseGrpcFile(text);
  expect(parsed.target).toBe("http://127.0.0.1:9");
  expect(parsed.metadata.map((r) => [r.name, r.value, r.enabled])).toEqual([["x-a", "1", true]]);
  expect(parsed.protoFiles).toEqual(["/p/echo.proto"]);
  expect(parsed.method).toBe("demo.Echo/Alpha");
  for (const bad of [
    "{",
    "[]",
    JSON.stringify({ version: 2, kind: "grpc", grpc: {} }),
    JSON.stringify({ version: 1, kind: "mcp" }),
  ])
    expect(() => parseGrpcFile(bad)).toThrow();
});
