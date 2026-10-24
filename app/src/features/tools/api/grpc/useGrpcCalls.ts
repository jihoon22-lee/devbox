/**
 * gRPC 호출 기록(화면 메모리, 최근 50개). 호출마다 요청 번호·메서드·스트림을 묶어 두어 다른 메서드를 고른 뒤
 * 도착한 응답도 그 호출에 붙고, [취소]는 그 호출의 스트림만 닫는다(AR-F08). 화면을 떠나면 모두 닫는다.
 */
import { useEffect, useRef, useState } from "react";
import type { GrpcCall } from "../../../../rpc/gen/rpc";
import { SendFailure, useApiBackend } from "../backend";
import { applyGrpc, type CallRecord, newCall } from "./grpcDrafts";

const MAX_CALLS = 50;

export function useGrpcCalls() {
  const backend = useApiBackend();
  const [calls, setCalls] = useState<CallRecord[]>([]);
  const streams = useRef(new Map<number, { close(): void }>());
  const next = useRef(0);
  useEffect(() => {
    const open = streams.current;
    return () => {
      for (const stream of open.values()) stream.close();
      open.clear();
    };
  }, []);
  const update = (requestId: number, change: (call: CallRecord) => CallRecord) =>
    setCalls((list) => list.map((call) => (call.requestId === requestId ? change(call) : call)));

  async function start(params: GrpcCall) {
    const requestId = ++next.current;
    setCalls((list) => [newCall(requestId, params.target, params.method, Date.now()), ...list].slice(0, MAX_CALLS));
    try {
      const stream = await backend.openGrpc(params, {
        onEvent: (event) => update(requestId, (call) => applyGrpc(call, event)),
        onEnd: () => {
          streams.current.delete(requestId);
          update(requestId, (call) =>
            call.status === "running" ? { ...call, status: "failed", failure: { code: "connection_lost" } } : call,
          );
        },
      });
      streams.current.set(requestId, stream);
    } catch (error) {
      const failure =
        error instanceof SendFailure ? { code: error.code, detail: error.detail } : { code: "internal" as const };
      update(requestId, (call) => ({ ...call, status: "failed", failure }));
    }
  }

  function cancel(requestId: number) {
    update(requestId, (call) => (call.status === "running" ? { ...call, status: "cancelled" } : call));
    const stream = streams.current.get(requestId);
    streams.current.delete(requestId);
    stream?.close();
  }

  return { calls, start, cancel };
}
