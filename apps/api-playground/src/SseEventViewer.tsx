import { useState } from "react";
import type { SseEvent } from "./lib/sse";

interface SseEventViewerProps {
  events: readonly SseEvent[];
  dropped: number;
  paused: boolean;
  onPauseChange: (paused: boolean) => void;
  onError: (message: string) => void;
}

export function formatSseEvents(events: readonly SseEvent[]): string {
  return events.map((event, index) => {
    const metadata = [
      `event: ${event.event}`,
      event.id ? `id: ${event.id}` : null,
      event.retryMs === undefined ? null : `retry: ${event.retryMs}`,
    ].filter(Boolean).join("\n");
    return `# ${index + 1}\n${metadata}\n\n${event.data}`;
  }).join("\n\n");
}

export function SseEventViewer({
  events,
  dropped,
  paused,
  onPauseChange,
  onError,
}: SseEventViewerProps) {
  const [copying, setCopying] = useState(false);

  const copyEvents = async () => {
    if (copying || events.length === 0) return;
    setCopying(true);
    try {
      await navigator.clipboard.writeText(formatSseEvents(events));
    } catch {
      onError("SSE event를 클립보드에 복사하지 못했습니다.");
    } finally {
      setCopying(false);
    }
  };

  return (
    <section className="sse-viewer" aria-labelledby="sse-events-heading">
      <div className="sse-viewer-head">
        <strong id="sse-events-heading">SSE 이벤트</strong>
        <span className="dim">{events.length}개 표시{dropped ? ` · ${dropped}개 제외됨` : ""}</span>
        <span className="spacer" />
        <label className="toggle">
          <input
            type="checkbox"
            checked={paused}
            onChange={(event) => onPauseChange(event.currentTarget.checked)}
            aria-label="SSE 이벤트 렌더링 일시 중지"
          />
          렌더링 일시 중지
        </label>
        <button
          type="button"
          className="btn"
          disabled={copying || events.length === 0}
          onClick={() => void copyEvents()}
        >
          {copying ? "복사 중…" : "마스킹된 이벤트 복사"}
        </button>
      </div>
      <div
        className="sse-event-list"
        role="log"
        aria-live={paused ? "off" : "polite"}
        aria-label="SSE 이벤트 스트림"
      >
        {events.length === 0 ? (
          <div className="response-empty">SSE 스트림을 시작하면 이벤트가 표시됩니다.</div>
        ) : events.map((event, index) => (
          <article className="sse-event-row" key={`${index}-${event.event}-${event.id ?? ""}`}>
            <div className="sse-event-meta">
              <span className="sse-event-name">{event.event}</span>
              {event.id && <span className="dim">id: {event.id}</span>}
              {event.retryMs !== undefined && <span className="dim">retry: {event.retryMs}ms</span>}
            </div>
            <pre className="sse-event-data">{event.data || " "}</pre>
          </article>
        ))}
      </div>
    </section>
  );
}
