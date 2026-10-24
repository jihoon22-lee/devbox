import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";

export interface ActivityState {
  consent: boolean;
  paused: boolean;
  legacyKnowledge: boolean;
}
export function useNativeActivity() {
  const [state, setState] = useState<ActivityState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const pending = useRef(false);
  const revision = useRef(0);
  // biome-ignore lint/correctness/useExhaustiveDependencies: attempt explicitly reloads native state.
  useEffect(() => {
    let cancelled = false;
    let off: (() => void) | undefined;
    setError(null);
    void listen<ActivityState>("activity://state", (event) => {
      if (!cancelled) {
        revision.current += 1;
        setState(event.payload);
      }
    })
      .then((unlisten) => {
        if (cancelled) unlisten();
        else off = unlisten;
      })
      .catch(() => {});
    const expected = revision.current;
    void invoke<ActivityState>("activity_state")
      .then((value) => {
        if (!cancelled && expected === revision.current) setState(value);
      })
      .catch(() => {
        if (!cancelled && expected === revision.current) setError("활동 기록 상태를 확인하지 못했습니다.");
      });
    return () => {
      cancelled = true;
      off?.();
    };
  }, [attempt]);
  const change = async (next: { consent?: boolean; paused?: boolean }) => {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      const expected = ++revision.current;
      const value = await invoke<ActivityState>("activity_set", next);
      if (revision.current === expected) setState(value);
    } catch {
      // The native side may have stopped anyway (a withdrawal stops before saving).
      const expected = revision.current;
      const now = await invoke<ActivityState>("activity_state").catch(() => null);
      if (now && expected === revision.current) setState(now);
      setError(
        next.consent === false && now && !now.consent
          ? "활동 기록을 껐지만 설정을 저장하지 못했습니다. 앱을 다시 열면 다시 켜질 수 있으니 다시 시도하세요."
          : "활동 기록 설정을 바꾸지 못했습니다. 다시 시도하세요.",
      );
    } finally {
      pending.current = false;
      setBusy(false);
    }
  };
  return { state, error, busy, change, retry: () => setAttempt((value) => value + 1) };
}
export type ActivityControl = ReturnType<typeof useNativeActivity>;
