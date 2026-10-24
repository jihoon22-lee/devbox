import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import { Button } from "../../ui";

/** `activity_state`, also delivered as `activity://state` when the tray changes it. */
export interface ActivityState {
  consent: boolean;
  paused: boolean;
  legacyKnowledge: boolean;
}

/** Consent first (01 §6.2): before it only the collect button, and no collector runs. */
export function ActivityConsent() {
  const [state, setState] = useState<ActivityState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const pending = useRef(false);
  // biome-ignore lint/correctness/useExhaustiveDependencies: attempt explicitly reloads native state.
  useEffect(() => {
    let cancelled = false;
    let off: (() => void) | undefined;
    setError(null);
    void listen<ActivityState>("activity://state", (event) => {
      if (!cancelled) setState(event.payload);
    })
      .then((unlisten) => {
        if (cancelled) unlisten();
        else off = unlisten;
      })
      .catch(() => {});
    void invoke<ActivityState>("activity_state")
      .then((value) => {
        if (!cancelled) setState(value);
      })
      .catch(() => {
        if (!cancelled) setError("활동 기록 상태를 확인하지 못했습니다.");
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
      setState(await invoke<ActivityState>("activity_set", next));
    } catch {
      // The native side may have stopped anyway (a withdrawal stops before saving).
      const now = await invoke<ActivityState>("activity_state").catch(() => null);
      if (now) setState(now);
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
  return (
    <section aria-label="활동 기록">
      <h3>활동 기록</h3>
      {state && !state.consent && (
        <>
          <p>켜면 앞에 있는 창의 앱 이름과 제목을 기록합니다. 제목은 개인정보 규칙을 적용한 뒤에만 보냅니다.</p>
          {state.legacyKnowledge && <p>v0.9.0 Knowledge의 활동 기록이 켜져 있다면 한쪽만 켜 두세요</p>}
          <Button disabled={busy} onClick={() => void change({ consent: true })}>
            활동 기록 켜기
          </Button>
        </>
      )}
      {state?.consent && (
        <>
          <p>{state.paused ? "활동 기록을 일시중지했습니다." : "활동을 기록하고 있습니다."}</p>
          <Button disabled={busy} onClick={() => void change({ paused: !state.paused })}>
            {state.paused ? "다시 시작" : "일시중지"}
          </Button>{" "}
          <Button disabled={busy} onClick={() => void change({ consent: false })}>
            활동 기록 끄기
          </Button>
        </>
      )}
      {error && (
        <p role="alert" style={{ color: "var(--danger)" }}>
          {error}
          {!state && <Button onClick={() => setAttempt((value) => value + 1)}>다시 시도</Button>}
        </p>
      )}
    </section>
  );
}
