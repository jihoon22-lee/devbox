import { formatHandoffExpiry } from "../lib/requestPresentation";
import type * as React from "react";

interface Props {
  handoffDialogRef: React.RefObject<HTMLElement | null>;
  handoffPreview: import("../../generated/ApiRequestHandoffPreview").ApiRequestHandoffPreview;
  handoffCancelButtonRef: React.RefObject<HTMLButtonElement | null>;
  handoffBusy: boolean;
  onCancelHandoff: () => Promise<void>;
  onApplyHandoff: () => Promise<void>;
}

export function RequestHandoffDialog({
  handoffDialogRef,
  handoffPreview,
  handoffCancelButtonRef,
  handoffBusy,
  onCancelHandoff,
  onApplyHandoff,
}: Props) {
  return (
    <div className="handoff-backdrop">
      <section
        className="handoff-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="handoff-dialog-title"
        aria-describedby="handoff-dialog-description"
        ref={handoffDialogRef}
      >
        <div className="handoff-dialog-head">
          <div>
            <h2 id="handoff-dialog-title">
              {handoffPreview.producerId === "developer-toolbox"
                ? "Toolbox 텍스트 요청 미리보기"
                : "Webhook 요청 미리보기"}
            </h2>
            <p id="handoff-dialog-description" className="handoff-subtitle">
              적용하기 전 요청을 확인하세요. origin-form URL은 적용 후 request editor에서 host를 입력할 수 있습니다.
              secret 원문은 전달되지 않고 환경 변수 참조만 보존됩니다. 적용은 편집기에 요청을 넣기만 하며 자동으로
              전송하지 않습니다.
            </p>
          </div>
          <span className="handoff-kind">{handoffPreview.kind}</span>
        </div>
        <dl className="handoff-meta">
          <div>
            <dt>producer</dt>
            <dd>{handoffPreview.producerId}</dd>
          </div>
          <div>
            <dt>consumer</dt>
            <dd>{handoffPreview.consumerId}</dd>
          </div>
          <div>
            <dt>handoff</dt>
            <dd>
              <code>{handoffPreview.handoffId}</code>
            </dd>
          </div>
          <div>
            <dt>expires</dt>
            <dd>{formatHandoffExpiry(handoffPreview.expiresAtMs)}</dd>
          </div>
        </dl>
        <div className="handoff-request-preview">
          <div className="handoff-request-line">
            <strong>{handoffPreview.request.method}</strong>
            <code>{handoffPreview.request.url}</code>
          </div>
          {handoffPreview.request.headers.length > 0 && (
            <div className="handoff-header-preview">
              {handoffPreview.request.headers.map((header, index) => (
                <div className="handoff-header-line" key={`${header.key}-${index}`}>
                  <span>{header.key}</span>
                  <code>{header.value}</code>
                </div>
              ))}
            </div>
          )}
          {handoffPreview.request.body && <pre className="handoff-body-preview">{handoffPreview.request.body}</pre>}
        </div>
        <div className="handoff-dialog-actions">
          <button
            ref={handoffCancelButtonRef}
            type="button"
            className="btn"
            disabled={handoffBusy}
            onClick={() => void onCancelHandoff()}
          >
            취소
          </button>
          <button type="button" className="btn send" disabled={handoffBusy} onClick={() => void onApplyHandoff()}>
            {handoffBusy ? "처리 중..." : "적용"}
          </button>
        </div>
      </section>
    </div>
  );
}
