import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useRef, useState } from "react";
import { appPreferences, isNative, setAutostart } from "../features/settings/nativePreferences";
import { Button, ConfirmDialog } from "../ui";
import { FirstRun } from "./FirstRun";
import { useNativeRuntime } from "./nativeRuntime";

/** Mount outside ConnectionGate so tray quit is always available, including startup failures. */
export function NativeMainController({ ready }: { ready: boolean }) {
  const [main] = useState(() => isNative() && getCurrentWindow().label === "main");
  const [firstRun, setFirstRun] = useState(false);
  const [quit, setQuit] = useState<number | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [busy, setBusy] = useState(false);
  const firstRunChecked = useRef(false);
  const pending = useRef(false);
  const exit = useCallback(async () => {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
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
    const requestQuit = () => {
      const snapshot = useNativeRuntime.getState();
      const count = snapshot.snapshotCurrent || (snapshot.openTasks ?? 0) > 0 ? snapshot.openTasks : null;
      if (count === 0) void exit();
      else {
        setFirstRun(false);
        setQuit(count);
      }
    };
    void (async () => {
      try {
        for (const [name, receive] of [
          ["devbox://palette", () => useNativeRuntime.getState().push({ kind: "palette" })],
          [
            "devbox://bottom-terminal",
            (payload: { wasFocused: boolean }) =>
              useNativeRuntime.getState().push({ kind: "bottom-terminal", wasFocused: payload.wasFocused === true }),
          ],
          ["devbox://quit", requestQuit],
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
          onCancel={() => setFirstRun(false)}
          onDone={async ({ autostart }) => {
            await setAutostart(autostart, true);
            setFirstRun(false);
          }}
        />
      )}
      <ConfirmDialog
        open={quit !== undefined}
        title="devbox 종료"
        body={`${quit === null ? "실행 중인 에이전트 작업을 확인하지 못했습니다." : `에이전트 ${quit ?? 0}개가 실행 중입니다.`} 앱을 끄면 WSL이 약 15초 뒤 멈추면서 함께 종료될 수 있습니다.`}
        confirmLabel="종료"
        disabled={busy}
        onCancel={() => !pending.current && setQuit(undefined)}
        onConfirm={() => void exit()}
      />
    </>
  );
}
