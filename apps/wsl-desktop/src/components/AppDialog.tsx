import { useCallback, useEffect, useRef, useState } from "react";
import { isImeComposing, restoreFocus, trapDialogKeyDown } from "@devbox/a11y";

export interface DialogRequest {
  kind: "confirm" | "prompt";
  title: string;
  /** 본문 문단. 사용자 입력 원문이 아니라 앱이 만든 고정 문장만 넣는다. */
  lines?: readonly string[];
  /** 실행 직전 최종 문자열처럼 그대로 보여 줘야 하는 값 (monospace, 스크롤). */
  detail?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  defaultValue?: string;
  placeholder?: string;
  inputLabel?: string;
  maxLength?: number;
  /** 있으면 확인 창에 기억 체크박스를 추가한다. 저장 여부는 호출자가 정한다. */
  rememberLabel?: string;
}

export interface DialogAnswer {
  confirmed: boolean;
  value: string;
  remember: boolean;
}

interface Draft {
  id: number;
  value: string;
  remember: boolean;
}

export interface PendingDialog {
  id: number;
  request: DialogRequest;
  resolve: (answer: DialogAnswer) => void;
}

export type AskDialog = (request: DialogRequest) => Promise<DialogAnswer>;

const CANCELLED: DialogAnswer = { confirmed: false, value: "", remember: false };
const MAX_PROMPT_LENGTH = 4096;

/**
 * 앱 안에서 확인·입력을 받는다. native `window.confirm`/`prompt`는 테마·focus 복원·IME를
 * 앱과 공유하지 않고 WebView를 통째로 멈추므로 여기서 대체한다.
 *
 * 요청은 FIFO로 직렬화한다 — 팬 두 곳이 동시에 물어도 하나씩 순서대로 뜬다. 언마운트
 * 시점에 남아 있는 요청은 취소로 resolve해 호출자가 영원히 기다리지 않게 한다.
 */
export function useAppDialog(): {
  ask: AskDialog;
  pending: PendingDialog | null;
  answer: (answer: DialogAnswer) => void;
} {
  const [pending, setPending] = useState<PendingDialog | null>(null);
  const nextId = useRef(0);
  const chain = useRef<Promise<unknown>>(Promise.resolve());
  const outstanding = useRef(new Set<(answer: DialogAnswer) => void>());
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      for (const resolve of outstanding.current) resolve(CANCELLED);
      outstanding.current.clear();
    };
  }, []);

  const ask = useCallback<AskDialog>((request) => {
    const run = chain.current.then(() => new Promise<DialogAnswer>((resolve) => {
      if (!mounted.current) {
        resolve(CANCELLED);
        return;
      }
      outstanding.current.add(resolve);
      setPending({ id: ++nextId.current, request, resolve });
    }));
    chain.current = run.then(() => undefined, () => undefined);
    return run;
  }, []);

  const answer = useCallback((result: DialogAnswer) => {
    setPending((current) => {
      if (current) {
        outstanding.current.delete(current.resolve);
        current.resolve(result);
      }
      return null;
    });
  }, []);

  return { ask, pending, answer };
}

interface AppDialogProps {
  pending: PendingDialog | null;
  onAnswer: (answer: DialogAnswer) => void;
}

export default function AppDialog({ pending, onAnswer }: AppDialogProps) {
  const dialogRef = useRef<HTMLElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const confirmRef = useRef<HTMLButtonElement>(null);
  const request = pending?.request ?? null;
  const dialogId = pending?.id ?? 0;
  const isPrompt = request?.kind === "prompt";
  // Draft is derived during render rather than reset from an effect: a new request must show
  // its default value on the first paint, not one commit later.
  const [draft, setDraft] = useState<Draft>({ id: 0, value: "", remember: false });
  const current: Draft = draft.id === dialogId
    ? draft
    : { id: dialogId, value: request?.defaultValue ?? "", remember: false };
  const { value, remember } = current;

  useEffect(() => {
    if (!dialogId) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const frame = requestAnimationFrame(() => {
      if (isPrompt) {
        inputRef.current?.focus();
        inputRef.current?.select();
      } else {
        confirmRef.current?.focus();
      }
    });
    return () => {
      cancelAnimationFrame(frame);
      restoreFocus(opener);
    };
    // 요청 identity가 바뀔 때만 focus를 옮긴다. 입력 중 재실행하면 커서가 튄다.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dialogId]);

  if (!request) return null;

  const cancel = () => onAnswer({ confirmed: false, value: "", remember: false });
  const confirm = () => onAnswer({ confirmed: true, value, remember });

  return (
    <div
      className="dialog-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) cancel();
      }}
    >
      <section
        ref={dialogRef}
        className={`app-dialog ${request.danger ? "danger" : ""}`}
        role="alertdialog"
        aria-modal="true"
        aria-label={request.title}
        onKeyDown={(event) => {
          if (dialogRef.current) trapDialogKeyDown(event, dialogRef.current, cancel);
        }}
      >
        <h2 className="dialog-title">{request.title}</h2>
        {request.lines?.map((line) => (
          <p key={line} className="dialog-line">{line}</p>
        ))}
        {request.detail !== undefined && <pre className="dialog-detail">{request.detail}</pre>}
        {isPrompt && (
          <input
            ref={inputRef}
            className="dialog-input"
            aria-label={request.inputLabel ?? request.title}
            placeholder={request.placeholder}
            maxLength={request.maxLength ?? MAX_PROMPT_LENGTH}
            value={value}
            onChange={(event) => setDraft({ ...current, value: event.currentTarget.value })}
            onKeyDown={(event) => {
              if (isImeComposing(event) || event.key !== "Enter") return;
              event.preventDefault();
              confirm();
            }}
          />
        )}
        {request.rememberLabel && (
          <label className="dialog-remember">
            <input
              type="checkbox"
              checked={remember}
              onChange={(event) => setDraft({ ...current, remember: event.currentTarget.checked })}
            />
            {request.rememberLabel}
          </label>
        )}
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={cancel}>
            {request.cancelLabel ?? "취소"}
          </button>
          <button
            ref={confirmRef}
            type="button"
            className={`btn primary ${request.danger ? "danger" : ""}`}
            onClick={confirm}
          >
            {request.confirmLabel ?? "확인"}
          </button>
        </div>
      </section>
    </div>
  );
}
