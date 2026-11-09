import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useRef, useState } from "react";
import { appPreferences, isNative, markAutostartAsked, setAutostart } from "../features/settings/nativePreferences";
import { Button } from "../ui";
import { FirstRun } from "./FirstRun";
import { useNativeRuntime } from "./nativeRuntime";
import { QuitConfirmation, type QuitReasons } from "./QuitConfirmation";

/** The draft lives only in the hidden capture window; a failed question counts as none. */
const captureHasDraft = () =>
  invoke<boolean>("capture_has_draft").then(
    (draft) => draft === true,
    () => false,
  );

/** Mount outside ConnectionGate so tray quit is always available, including startup failures. */
export function NativeMainController({ ready }: { ready: boolean }) {
  const [main] = useState(() => isNative() && getCurrentWindow().label === "main");
  const [firstRun, setFirstRun] = useState(false);
  const [quit, setQuit] = useState<QuitReasons | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [busy, setBusy] = useState(false);
  const firstRunChecked = useRef(false);
  const pending = useRef(false);
  /** One quit at a time. `recheck` first asks again whether quick capture holds a draft now. */
  const exit = useCallback(async (recheck = false) => {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      // Capture stays open to the user while the dialog asks: a draft written meanwhile is asked about.
      if (recheck && (await captureHasDraft())) {
        setQuit((reasons) => reasons && { ...reasons, captureDraft: true });
        return;
      }
      await invoke("quit_app");
    } catch {
      setError("종료하지 못했습니다. 다시 시도하세요.");
    } finally {
      pending.current = false;
      setBusy(false);
    }
  }, []);
  // biome-ignore lint/correctness/useExhaustiveDependencies: attempt explicitly retries native listener registration.
  useEffect(() => {
    if (!main) return;
    let cancelled = false;
    let lease: number | undefined;
    const cleanups: (() => void)[] = [];
    const requestQuit = async () => {
      const captureDraft = await captureHasDraft();
      if (cancelled) return;
      const snapshot = useNativeRuntime.getState();
      const tasks = snapshot.snapshotCurrent || (snapshot.openTasks ?? 0) > 0 ? snapshot.openTasks : null;
      const reasons = { tasks, captureDraft };
      setFirstRun(false);
      setQuit(reasons);
    };
    void (async () => {
      try {
        for (const [name, receive] of [
          [
            "devbox://palette",
            (payload: { traceId?: unknown } | null) =>
              useNativeRuntime
                .getState()
                .push(
                  typeof payload?.traceId === "number"
                    ? { kind: "palette", traceId: payload.traceId }
                    : { kind: "palette" },
                ),
          ],
          [
            "devbox://bottom-terminal",
            (payload: { wasFocused: boolean }) =>
              useNativeRuntime.getState().push({ kind: "bottom-terminal", wasFocused: payload.wasFocused === true }),
          ],
          ["devbox://quit", () => void requestQuit()],
        ] as const) {
          const off = await listen<{ wasFocused: boolean }>(name, (event) => {
            if (!cancelled) receive(event.payload);
          });
          if (cancelled) {
            off();
            return;
          }
          cleanups.push(off);
        }
        lease = await invoke<number>("app_intents_ready");
        if (cancelled && lease !== undefined) void invoke("app_intents_not_ready", { leaseId: lease }).catch(() => {});
      } catch {
        if (!cancelled) setError("트레이 명령을 준비하지 못했습니다. 앱을 다시 여세요.");
      }
    })();
    return () => {
      cancelled = true;
      cleanups.forEach((off) => off());
      if (lease !== undefined) void invoke("app_intents_not_ready", { leaseId: lease }).catch(() => {});
    };
  }, [main, attempt, exit]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: attempt explicitly retries the native preference read.
  useEffect(() => {
    if (!main || !ready || firstRunChecked.current) return;
    setError(null);
    let cancelled = false;
    void appPreferences()
      .then((preferences) => {
        if (cancelled) return;
        firstRunChecked.current = true;
        if (preferences && !preferences.autostartAsked) setFirstRun(true);
      })
      .catch(() => {
        if (!cancelled) setError("앱 설정을 확인하지 못했습니다.");
      });
    return () => {
      cancelled = true;
    };
  }, [main, ready, attempt]);
  if (!main) return null;
  return (
    <>
      {error && (
        <p role="alert">
          {error}
          <Button
            onClick={() => {
              setError(null);
              setAttempt((value) => value + 1);
            }}
          >
            다시 시도
          </Button>
        </p>
      )}
      {firstRun && quit === undefined && (
        <FirstRun
          onCancel={() => {
            setFirstRun(false);
            void markAutostartAsked().catch(() => {
              // Nothing was saved, so [다시 시도] reads the settings again and asks again.
              firstRunChecked.current = false;
              setError("첫 실행 선택을 저장하지 못했습니다. [다시 시도]를 누르면 선택 창을 다시 엽니다.");
            });
          }}
          onDone={async ({ autostart }) => {
            await setAutostart(autostart, true);
            setFirstRun(false);
          }}
        />
      )}
      <QuitConfirmation
        reasons={quit}
        busy={busy}
        onCancel={() => !pending.current && setQuit(undefined)}
        onOpenCapture={() => {
          if (pending.current) return;
          setQuit(undefined);
          // Native logs a failure to show the window, as for the tray item; nothing quits either way.
          void invoke("capture_open").catch(() => {});
        }}
        onConfirm={() => void exit(quit?.captureDraft === false)}
      />
    </>
  );
}
