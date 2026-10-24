import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useRef, useState } from "react";
import { RpcError } from "../../rpc/client";
import { messageFor } from "../../rpc/messages";
import { useRpcMutation } from "../../rpc/query";
import { useRpcSession } from "../../rpc/RpcProvider";
import { Button } from "../../ui";
import { captureKey, hideCapture, readCaptureDraft, saveCaptureDraft, submitCapture } from "./capture";

export function CaptureWindow() {
  const { state } = useRpcSession();
  const append = useRpcMutation("notes.append");
  const [text, setText] = useState(readCaptureDraft);
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [ready, setReady] = useState(false);
  const pending = useRef(false);
  const input = useRef<HTMLTextAreaElement>(null);
  const change = (value: string) => {
    setText(value);
    if (!saveCaptureDraft(value)) setError("임시 내용을 보관하지 못했습니다. 이 창을 닫지 말고 저장해 주세요.");
  };
  useEffect(() => {
    let live = true;
    let frame = 0;
    let off: (() => void) | undefined;
    let offFocus: (() => void) | undefined;
    const shown = () => {
      if (document.visibilityState === "hidden") return;
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        if (!live) return;
        input.current?.focus();
        setReady(true);
        frame = requestAnimationFrame(() => {
          if (live && "__TAURI_INTERNALS__" in window) void invoke("capture_ready").catch(() => {});
        });
      });
    };
    document.addEventListener("visibilitychange", shown);
    if ("__TAURI_INTERNALS__" in window) {
      void getCurrentWindow()
        .onFocusChanged((event) => {
          if (event.payload) shown();
        })
        .then((stop) => {
          if (live) offFocus = stop;
          else stop();
        })
        .catch(() => {});
      void listen("capture://show", shown)
        .then((stop) => {
          if (live) off = stop;
          else stop();
        })
        .catch(() => {});
    }
    shown();
    return () => {
      live = false;
      cancelAnimationFrame(frame);
      off?.();
      offFocus?.();
      document.removeEventListener("visibilitychange", shown);
    };
  }, []);
  const hide = async () => {
    if (!saveCaptureDraft(text)) {
      setError("임시 내용을 보관하지 못했습니다. 이 창을 닫지 말고 저장해 주세요.");
      return;
    }
    try {
      await hideCapture();
    } catch {
      setError("빠른 캡처 창을 숨기지 못했습니다.");
    }
  };
  const submit = async () => {
    if (pending.current || !text.trim() || state !== "ready") return;
    pending.current = true;
    setBusy(true);
    setError(undefined);
    try {
      await submitCapture(text, append.mutateAsync, hideCapture, change);
    } catch (failure) {
      const e = failure instanceof RpcError ? failure : new RpcError("internal");
      setError(messageFor(e.code, e.detail).title);
    } finally {
      pending.current = false;
      setBusy(false);
    }
  };
  return (
    <main
      data-capture-ready={ready ? "true" : "false"}
      style={{ padding: "var(--space-4)", height: "100vh", overflow: "auto", boxSizing: "border-box" }}
    >
      <header data-tauri-drag-region>빠른 캡처</header>
      <textarea
        ref={input}
        aria-label="캡처할 내용"
        rows={4}
        value={text}
        disabled={busy}
        onChange={(event) => change(event.currentTarget.value)}
        style={{ width: "100%", resize: "none" }}
        onKeyDown={(event) => {
          const action = captureKey({
            key: event.key,
            shiftKey: event.shiftKey,
            isComposing: event.nativeEvent.isComposing,
            keyCode: event.keyCode,
          });
          if (action === "none") return;
          event.preventDefault();
          if (event.repeat || busy) return;
          if (action === "hide") void hide();
          else void submit();
        }}
      />
      <p>Enter로 저장 · Shift+Enter로 줄바꿈 · Esc로 숨기기</p>
      <Button disabled={busy || state !== "ready" || !text.trim()} onClick={() => void submit()}>
        {busy ? "저장 중…" : "저장"}
      </Button>
      {state !== "ready" && <p role="status">데몬에 연결되면 저장할 수 있습니다. 입력한 내용은 유지됩니다.</p>}
      {error && <p role="alert">{error}</p>}
    </main>
  );
}
