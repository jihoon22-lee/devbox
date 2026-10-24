/**
 * 도구 화면이 데몬에 바라는 것(컬렉션 파일·송신·비밀 저장). 화면 시험은 가짜 구현을 넣는다(`test-backend.ts`).
 * 실제 구현은 `rpcBackend.ts`(RPC 클라이언트)다.
 */
import { createContext, type ReactNode, useContext } from "react";
import type {
  CollectionExpect,
  CollectionList,
  CollectionRoot,
  FileText,
  GrpcCall,
  GrpcCatalog,
  GrpcEvent,
  GrpcTarget,
  HistoryPage,
  Pair,
  RealtimeEvent,
  RequestOrigin,
  SendRequest,
  SseOpen,
  Written,
  WsInput,
  WsOpen,
} from "../../../rpc/gen/rpc";

export interface SentResponse {
  historyId: string | null;
  sentAtMs: number;
  status: number;
  reason: string;
  httpVersion: string;
  headers: Pair[];
  /** 화면 메모리에는 10 MiB까지(넘으면 `truncated`). */
  body: Uint8Array;
  truncated: boolean;
  elapsedMs: number;
  sizeBytes: number;
  binary: boolean;
}

/** 송신 전 거절(`api.invalid_header` 등)이나 송신 중 실패. `detail`은 오류 표의 세부다. */
export class SendFailure extends Error {
  constructor(
    readonly code: string,
    readonly detail?: unknown,
  ) {
    super(code);
    this.name = "SendFailure";
  }
}

export interface ApiBackend {
  list(root: CollectionRoot): Promise<CollectionList>;
  read(root: CollectionRoot, path: string): Promise<FileText>;
  write(root: CollectionRoot, path: string, text: string, expect: CollectionExpect): Promise<Written>;
  /** `api.collection_delete`는 확인 정책 always다. 부르는 쪽이 먼저 확인 창을 보인다. */
  remove(root: CollectionRoot, path: string, digest: string): Promise<void>;
  review(root: CollectionRoot, path: string, digest: string): Promise<void>;
  send(request: SendRequest, signal: AbortSignal): Promise<SentResponse>;
  history(origin: RequestOrigin, limit: number): Promise<HistoryPage>;
  historyBody(id: string): Promise<string>;
  secretNames(): Promise<string[]>;
  setSecret(name: string, value: string): Promise<void>;
  /** 연결 전 검사 실패는 `SendFailure`로 거절한다(연결 실패는 `failed` 이벤트). */
  openWs(params: WsOpen, handlers: RealtimeHandlers): Promise<RealtimeSession>;
  openSse(params: SseOpen, handlers: RealtimeHandlers): Promise<RealtimeSession>;
  /** gRPC 서비스 목록(reflection 또는 proto 파일). 연결 전 검사 실패는 `SendFailure`. */
  grpcDescribe(params: GrpcTarget): Promise<GrpcCatalog>;
  /** gRPC 호출 하나. `close`는 호출 스트림을 닫는다(데몬이 호출을 취소). */
  openGrpc(params: GrpcCall, handlers: LineHandlers<GrpcEvent>): Promise<{ close(): void }>;
}

/** 스트림 하나의 NDJSON 이벤트 받기. */
export interface LineHandlers<E> {
  onEvent(event: E): void;
  /** 데몬이 스트림을 끝냈거나(FIN) 화면이 닫음. */
  onEnd(): void;
}

/** WebSocket·SSE 세션 화면 쪽. `close`는 스트림을 닫는다(데몬이 연결을 끊고 다시 연결하지 않음). */
export type RealtimeHandlers = LineHandlers<RealtimeEvent>;
export interface RealtimeSession {
  send(input: WsInput): void;
  close(): void;
}

const Context = createContext<ApiBackend | null>(null);

export function ApiBackendProvider({ backend, children }: { backend: ApiBackend; children: ReactNode }) {
  return <Context.Provider value={backend}>{children}</Context.Provider>;
}

export function useApiBackend(): ApiBackend {
  const backend = useContext(Context);
  if (!backend) throw new Error("ApiBackendProvider is missing");
  return backend;
}
