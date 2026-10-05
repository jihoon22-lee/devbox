import assert from "node:assert/strict";
import { createServer } from "node:http2";
import { once } from "node:events";
import { readFile, mkdir, stat, opendir } from "node:fs/promises";
import path from "node:path";
import {
  requireApiContext,
  button,
  textbox,
  select,
  scenario,
  expectText,
  until,
} from "./windows-api-user-flow-actions.mjs";

export async function grpcExportObservation(filename) {
  const exists = async (file) => {
    try {
      await stat(file);
      return true;
    } catch (error) {
      return error.code === "ENOENT" ? false : null;
    }
  };
  const parent = path.dirname(filename);
  let fileCount = null;
  try {
    fileCount = 0;
    let entries = 0;
    for await (const entry of await opendir(parent)) {
      if (entry.isFile()) fileCount++;
      if (++entries >= 64 || fileCount > 32) {
        fileCount = null;
        break;
      }
    }
  } catch {
    fileCount = null;
  }
  return {
    expectedExists: await exists(filename),
    parentExists: await exists(parent),
    fileCount,
    defaultNameExists: await exists(path.join(parent, "grpc-exchange.json")),
  };
}
export async function readExactGrpcExport(filename, observe, read = readFile, inspect = grpcExportObservation) {
  try {
    return await read(filename, "utf8");
  } catch (error) {
    try {
      observe(await inspect(filename));
    } catch {
      try {
        observe({ unavailable: true });
      } catch {
        /* Preserve the original read failure. */
      }
    }
    throw error;
  }
}

const varint = (value) => {
  const bytes = [];
  do {
    const next = value & 127;
    value >>>= 7;
    bytes.push(next | (value ? 128 : 0));
  } while (value);
  return Buffer.from(bytes);
};
const field = (number, value) => {
  const bytes = Buffer.isBuffer(value) ? value : Buffer.from(value);
  return Buffer.concat([varint(number * 8 + 2), varint(bytes.length), bytes]);
};
const scalar = (number, value) => Buffer.concat([varint(number * 8), varint(value)]);
const protobuf = (...fields) => Buffer.concat(fields);
const frame = (bytes) => {
  const prefix = Buffer.alloc(5);
  prefix.writeUInt32BE(bytes.length, 1);
  return Buffer.concat([prefix, bytes]);
};
const method = (name, client, server) =>
  field(
    2,
    protobuf(
      field(1, name),
      field(2, ".fixture.Message"),
      field(3, ".fixture.Message"),
      scalar(5, client),
      scalar(6, server),
    ),
  );
const descriptor = protobuf(
  field(1, "partial.proto"),
  field(2, "fixture"),
  field(
    4,
    protobuf(field(1, "Message"), field(2, protobuf(field(1, "text"), scalar(3, 1), scalar(4, 1), scalar(5, 9)))),
  ),
  field(
    6,
    protobuf(
      field(1, "Partial"),
      method("Unary", 0, 0),
      method("Server", 0, 1),
      method("Bidi", 1, 1),
      method("Client", 1, 0),
    ),
  ),
  field(12, "proto3"),
);

// The product resets both editor template and result in a selected-method effect.
// Observe that transition before filling; CDP key dispatch is not a React commit receipt.
export function grpcMethodReady(document, name) {
  const method = document.querySelector('[aria-label="gRPC method"]');
  const card = document.querySelector(".grpc-method-card code");
  const editor = document.querySelector('[aria-label="gRPC ProtoJSON request"]');
  try {
    return (
      method?.value === `fixture.Partial.${name}` &&
      card?.textContent === `fixture.Partial/${name}` &&
      !document.querySelector(".grpc-result") &&
      Array.isArray(JSON.parse(editor?.value)) === ["Bidi", "Client"].includes(name)
    );
  } catch {
    return false;
  }
}
export async function prepareGrpcInvocation(context, name) {
  await until(
    () => context.cdp.evaluate(`(${grpcMethodReady.toString()})(document, ${JSON.stringify(name)})`),
    `Current ${name} method/editor transition not committed`,
  );
  const request = ["Bidi", "Client"].includes(name) ? '[{"text":"fixture"}]' : '{"text":"fixture"}';
  await context.ui.fill(textbox("gRPC ProtoJSON request"), request);
  await until(
    () =>
      context.cdp.evaluate(
        `document.querySelector('[aria-label="gRPC ProtoJSON request"]')?.value === ${JSON.stringify(request)}`,
      ),
    `Current ${name} request input not committed`,
  );
  await context.ui.waitForTarget(button("RPC 호출"));
  await context.ui.click(button("RPC 호출"));
}

export async function createPartialGrpcFixture() {
  const server = createServer();
  const sessions = new Set();
  const requests = { Unary: 0, Server: 0, Bidi: 0, Client: 0 };
  server.on("session", (session) => {
    sessions.add(session);
    session.on("error", () => {});
    session.on("close", () => sessions.delete(session));
  });
  server.on("stream", (stream, headers) => {
    stream.on("error", () => {});
    const rpc = headers[":path"];
    const reflected = rpc.includes("grpc.reflection");
    const methodName = rpc.split("/").at(-1);
    if (Object.hasOwn(requests, methodName)) requests[methodName] = Math.min(100, requests[methodName] + 1);
    const partial = rpc.endsWith("/Server") || rpc.endsWith("/Bidi");
    stream.respond({ ":status": 200, "content-type": "application/grpc" }, { waitForTrailers: true });
    stream.on("wantTrailers", () =>
      stream.sendTrailers({ "grpc-status": partial ? "13" : "0", "grpc-message": partial ? "synthetic-terminal" : "" }),
    );
    let bytes = Buffer.alloc(0);
    stream.on("data", (chunk) => {
      if (!reflected) return;
      bytes = Buffer.concat([bytes, chunk]);
      while (bytes.length >= 5 && bytes.length >= 5 + bytes.readUInt32BE(1)) {
        const size = bytes.readUInt32BE(1),
          message = bytes.subarray(5, 5 + size);
        bytes = bytes.subarray(5 + size);
        const response = message.includes(Buffer.from([0x3a, 0]))
          ? field(6, field(1, field(1, "fixture.Partial")))
          : field(4, field(1, descriptor));
        stream.write(frame(protobuf(field(2, message), response)));
      }
    });
    stream.on("end", () => {
      if (reflected) {
        stream.end();
        return;
      }
      stream.write(frame(field(1, "first-synthetic")));
      if (partial) stream.write(frame(field(1, "second-synthetic")));
      // Allow message frames to flush before the terminal non-OK trailers.
      setTimeout(() => stream.end(), 30);
    });
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return {
    endpoint: `http://127.0.0.1:${server.address().port}`,
    requests,
    close: async () => {
      for (const session of sessions) session.destroy();
      await new Promise((resolve) => server.close(resolve));
    },
  };
}

export const SCENARIO_IDS = Object.freeze(["GRPC-01"]);

export async function run(context) {
  requireApiContext(context);
  const fixture = await createPartialGrpcFixture();
  try {
    let exportFailureObservation, exportPickerObservation, currentMethod;
    const result = await scenario(context, "GRPC-01", async (record) => {
      await context.ui.click(button("프로토콜"));
      await context.ui.waitForTarget({ role: "tab", name: "gRPC" });
      await context.ui.click({ role: "tab", name: "gRPC" });
      await select(context, "gRPC 스키마 소스", 1);
      await context.ui.fill(textbox("gRPC 엔드포인트"), fixture.endpoint);
      await context.ui.click(button("gRPC 연결"));
      await until(
        async () =>
          await context.cdp.evaluate("document.querySelector('[aria-label=\"gRPC method\"]')?.options.length===4"),
        "Reflection method list missing",
      );
      if ((await context.document("grpc_history"))?.value.entries.length) await context.ui.click(button("기록 지우기"));
      for (const name of ["Server", "Bidi", "Unary", "Client"]) {
        currentMethod = name;
        const index = await context.cdp.evaluate(
          `Array.from(document.querySelector('[aria-label="gRPC method"]').options).findIndex(option=>option.value.endsWith('.${name}'))`,
        );
        assert.ok(index >= 0);
        await select(context, "gRPC method", index);
        await prepareGrpcInvocation(context, name);
        await until(async () => {
          const latest = (await context.document("grpc_history"))?.value.entries[0];
          return (
            latest?.method === name &&
            latest.responseMessageCount === (["Server", "Bidi"].includes(name) ? 2 : 1) &&
            latest.status === (["Server", "Bidi"].includes(name) ? "INTERNAL" : "OK")
          );
        }, `Current ${name} native exchange summary not committed`);
        await expectText(context, "first-synthetic");
        if (["Server", "Bidi"].includes(name)) {
          await expectText(context, "2개 수신 후 INTERNAL 종료");
          await expectText(context, "second-synthetic");
          record(`Actual ${name} RPC displays both native messages alongside INTERNAL terminal status`);
          if (name === "Server") {
            await until(
              async () => (await context.document("grpc_history"))?.value.entries[0]?.responseMessageCount === 2,
              "Partial summary not committed",
            );
            const output = path.join(context.fixtureRoot, "user-flow-output");
            await mkdir(output, { recursive: true });
            const filename = path.join(output, "partial-server-grpc.json");
            const savedNotice = "gRPC summary를 저장했습니다. message body와 credential 정보는 포함하지 않았습니다.";
            const exactSavedNotice = () =>
              context.cdp.evaluate(
                `(() => { const node = document.querySelector('.grpc-notice'); return !!node && !node.closest('[hidden]') && node.textContent === ${JSON.stringify(savedNotice)}; })()`,
              );
            assert.equal(await exactSavedNotice(), false, "Export saved notice must not precede this export");
            await context.ui.click(button("요약 내보내기"));
            const picker = await context.saveFile(filename);
            exportPickerObservation = {
              filenameMatched: picker?.filenameMatched === true,
              chooserClosed: picker?.chooserClosed === true,
              dispatchAcknowledged: picker?.dispatchAcknowledged === true,
            };
            await until(exactSavedNotice, "Exact current gRPC export saved notice missing");
            const exported = JSON.parse(
              await readExactGrpcExport(filename, (observation) => {
                exportFailureObservation = observation;
              }),
            );
            assert.equal(exported.exchange.responseMessageCount, 2);
            assert.equal(exported.exchange.status, "INTERNAL");
            assert.ok(!JSON.stringify(exported).includes("first-synthetic"));
            assert.ok(!JSON.stringify(exported).includes(fixture.endpoint));
            record(
              "Real summary export uses owned native Save dialog; exported count/status match UI and exclude response payload/endpoint",
            );
          }
        } else {
          await until(
            async () =>
              await context.cdp.evaluate("document.querySelector('.grpc-result .grpc-status-ok')?.textContent==='OK'"),
            "Nonstreaming OK result missing",
          );
          record(`Actual ${name} RPC retains its existing single-response OK behavior`);
        }
      }
      const history = await context.document("grpc_history");
      assert.ok(history.value.entries.some((item) => item.status === "INTERNAL" && item.responseMessageCount === 2));
      record(
        "Persisted native summary count agrees with the displayed partial result and contains only summary metadata",
      );
      await context.ui.click(button("연결 해제"));
    });
    if (result.status === "FAIL" && currentMethod) {
      result.rpcFailureObservation = { expectedMethod: currentMethod, requests: { ...fixture.requests } };
      try {
        const latest = (await context.document("grpc_history"))?.value.entries[0];
        result.rpcFailureObservation.latest = latest
          ? {
              method: Object.hasOwn(fixture.requests, latest.method) ? latest.method : "other",
              status: ["OK", "INTERNAL", "DEADLINE_EXCEEDED", "CANCELLED"].includes(latest.status)
                ? latest.status
                : "other",
              responseMessageCount:
                Number.isInteger(latest.responseMessageCount) &&
                latest.responseMessageCount >= 0 &&
                latest.responseMessageCount <= 100
                  ? latest.responseMessageCount
                  : null,
            }
          : null;
        result.rpcFailureObservation.ui = await context.cdp.evaluate(`(() => ({
          selectedMethod: ["Unary", "Server", "Bidi", "Client"].find(name => document.querySelector('[aria-label="gRPC method"]')?.value === 'fixture.Partial.' + name) ?? "other",
          resultPresent: !!document.querySelector('.grpc-result'),
          validationPresent: !!document.querySelector('.grpc-validation'),
          errorPresent: !!document.querySelector('.grpc-lab .mcp-error')
        }))()`);
      } catch {
        result.rpcFailureObservation.unavailable = true;
      }
    }
    if (exportFailureObservation) result.exportFailureObservation = exportFailureObservation;
    if (exportPickerObservation) result.exportPickerObservation = exportPickerObservation;
    return [result];
  } finally {
    await fixture.close();
  }
}
