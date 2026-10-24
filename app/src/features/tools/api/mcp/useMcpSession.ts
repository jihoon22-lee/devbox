/**
 * MCP 연결 하나(화면 메모리). 연결하면 준비(`ready`) 뒤 도구 목록을 한 번 묻고, 요청마다 번호를 붙여 응답을
 * 그 요청에 묶는다. 새 연결·[연결 끊기]·화면 떠남은 이전 연결을 닫고 늦게 온 이벤트를 버린다.
 */
import { useEffect, useRef, useState } from "react";
import type { McpConnect, McpInput } from "../../../../rpc/gen/rpc";
import { type McpSession, SendFailure, useApiBackend } from "../backend";
import { applyMcp, initialMcp, type McpState, type Pending } from "./session";

type Without<T, K extends keyof T> = T extends unknown ? Omit<T, K> : never;
export type McpRequest = Without<McpInput, "id">;

export function useMcpSession() {
  const backend = useApiBackend();
  const [state, setState] = useState<McpState | null>(null);
  const session = useRef<McpSession | null>(null);
  const generation = useRef(0);
  const seq = useRef(0);
  const listed = useRef(false);

  useEffect(
    () => () => {
      generation.current += 1;
      session.current?.close();
      session.current = null;
    },
    [],
  );

  function send(input: McpRequest, pending: Pending) {
    const open = session.current;
    if (!open) return;
    const id = `m${++seq.current}`;
    setState((st) => st && { ...st, pending: { ...st.pending, [id]: pending } });
    open.send({ ...input, id } as McpInput);
  }

  // 준비되고 스트림이 붙은 뒤 도구 목록을 한 번 묻는다(상태가 바뀔 때마다 보지만 `listed`로 한 번만).
  // biome-ignore lint/correctness/useExhaustiveDependencies: `send`는 ref만 쓴다.
  useEffect(() => {
    if (state?.status !== "ready" || listed.current || !session.current) return;
    listed.current = true;
    send({ type: "list", what: "tools", cursor: null }, { kind: "list", what: "tools" });
  }, [state]);

  function close() {
    generation.current += 1;
    session.current?.close();
    session.current = null;
    setState((st) => (st && st.status !== "failed" ? { ...st, status: "closed" } : st));
  }

  async function connect(params: McpConnect) {
    close();
    const gen = generation.current;
    listed.current = false;
    setState(initialMcp());
    try {
      const opened = await backend.openMcp(params, {
        onEvent: (event) => gen === generation.current && setState((st) => st && applyMcp(st, event)),
        onEnd: () => gen === generation.current && setState((st) => st && applyMcp(st, { type: "closed" })),
      });
      if (gen !== generation.current) return opened.close();
      session.current = opened;
      // 준비 이벤트가 열림보다 먼저 왔을 수 있다: 상태를 다시 그려 목록 요청 효과를 돌린다.
      setState((st) => st && { ...st });
    } catch (error) {
      if (gen !== generation.current) return;
      const failure =
        error instanceof SendFailure ? { code: error.code, detail: error.detail } : { code: "internal" as const };
      setState((st) => ({ ...(st ?? initialMcp()), status: "failed", failure }));
    }
  }

  return { state, connect, send, close };
}
