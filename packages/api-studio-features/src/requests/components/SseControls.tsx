import { sseStateLabel } from "../lib/requestPresentation";
import type * as React from "react";

interface Props {
  onStartSse: () => Promise<void>;
  persistenceReady: boolean;
  sending: boolean;
  sseActive: boolean;
  contextActionBusy: boolean;
  req: import("../types").RequestTemplate;
  requestConfigurationError: string | null;
  sseState: "idle" | "connecting" | "connected" | "stopped" | "closed" | "error";
  onStopSse: () => Promise<void>;
  sseHandleRef: React.RefObject<import("../api").SseStreamHandle | null>;
  sseOptions: import("../../generated/SseOptions").SseOptions;
  setSseOptions: React.Dispatch<React.SetStateAction<import("../../generated/SseOptions").SseOptions>>;
}

export function SseControls({
  onStartSse,
  persistenceReady,
  sending,
  sseActive,
  contextActionBusy,
  req,
  requestConfigurationError,
  sseState,
  onStopSse,
  sseHandleRef,
  sseOptions,
  setSseOptions,
}: Props) {
  return (
    <div className="sse-controls" aria-label="SSE 스트림 제어">
      <div className="sse-control-actions">
        <button
          type="button"
          className="btn send"
          onClick={() => void onStartSse()}
          disabled={
            !persistenceReady ||
            sending ||
            sseActive ||
            contextActionBusy ||
            !req.url ||
            Boolean(requestConfigurationError) ||
            (req.method !== "GET" && req.method !== "POST")
          }
        >
          {sseState === "connecting" ? "SSE 연결 중…" : "SSE 시작"}
        </button>
        <button
          type="button"
          className="btn danger-outline"
          onClick={() => void onStopSse()}
          disabled={!sseActive && !sseHandleRef.current}
        >
          SSE 중지
        </button>
        <span className={`sse-status sse-status-${sseState}`} role="status" aria-live="polite">
          SSE {sseStateLabel(sseState)}
        </span>
      </div>
      <div className="sse-control-options">
        <label className="toggle">
          <input
            type="checkbox"
            checked={sseOptions.reconnect}
            disabled={sseActive}
            onChange={(event) => setSseOptions({ ...sseOptions, reconnect: event.currentTarget.checked })}
          />
          재연결 (최대 5회, 기본 꺼짐)
        </label>
        <label className="sse-number-field">
          연결 시간(ms)
          <input
            type="number"
            min={100}
            max={30_000}
            step={100}
            value={sseOptions.connectTimeoutMs}
            disabled={sseActive}
            onChange={(event) => setSseOptions({ ...sseOptions, connectTimeoutMs: Number(event.currentTarget.value) })}
            aria-label="SSE 연결 제한 시간(밀리초)"
          />
        </label>
        <label className="sse-number-field">
          유휴 시간(ms)
          <input
            type="number"
            min={100}
            max={300_000}
            step={100}
            value={sseOptions.idleTimeoutMs}
            disabled={sseActive}
            onChange={(event) => setSseOptions({ ...sseOptions, idleTimeoutMs: Number(event.currentTarget.value) })}
            aria-label="SSE 유휴 제한 시간(밀리초)"
          />
        </label>
        <label className="sse-number-field">
          전체 시간(ms)
          <input
            type="number"
            min={1_000}
            max={3_600_000}
            step={1_000}
            value={sseOptions.totalTimeoutMs}
            disabled={sseActive}
            onChange={(event) => setSseOptions({ ...sseOptions, totalTimeoutMs: Number(event.currentTarget.value) })}
            aria-label="SSE 전체 제한 시간(밀리초)"
          />
        </label>
      </div>
      <div className="sse-policy-note">
        네이티브 SSE는 재연결 중 Last-Event-ID를 전달하지 않습니다. 이벤트는 제한된 메모리에만 보관되며, 브라우저
        미리보기는 CORS를 따르고 리디렉션을 차단합니다.
      </div>
    </div>
  );
}
