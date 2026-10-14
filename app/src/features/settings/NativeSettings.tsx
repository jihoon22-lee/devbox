import { useEffect, useRef, useState } from "react";
import { Button } from "../../ui";
import {
  appPreferences,
  configureShortcuts,
  type NativePreferences,
  type ShortcutStatus,
  setAutostart,
  shortcutStatus,
} from "./nativePreferences";

export function NativeAutostartSettings() {
  const [preferences, setPreferences] = useState<NativePreferences | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const pending = useRef(false);
  // biome-ignore lint/correctness/useExhaustiveDependencies: attempt explicitly reloads native state.
  useEffect(() => {
    let cancelled = false;
    setError(null);
    void appPreferences()
      .then((value) => {
        if (!cancelled) setPreferences(value);
      })
      .catch(() => {
        if (!cancelled) setError("자동 시작 설정을 확인하지 못했습니다.");
      });
    return () => {
      cancelled = true;
    };
  }, [attempt]);
  const change = async (enabled: boolean) => {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      setPreferences(await setAutostart(enabled));
    } catch {
      setError("자동 시작 설정을 바꾸지 못했습니다. 다시 시도하세요.");
    } finally {
      pending.current = false;
      setBusy(false);
    }
  };
  return (
    <div>
      {preferences && (
        <label>
          <input
            type="checkbox"
            checked={preferences.autostart}
            disabled={busy}
            onChange={(event) => void change(event.target.checked)}
          />{" "}
          Windows 시작 시 devbox 실행
        </label>
      )}
      {error && (
        <p role="alert" style={{ color: "var(--danger)" }}>
          {error}
          {!preferences && <Button onClick={() => setAttempt((value) => value + 1)}>다시 시도</Button>}
        </p>
      )}
    </div>
  );
}

export function NativeShortcutSettings() {
  const [status, setStatus] = useState<ShortcutStatus | null>(null);
  const [palette, setPalette] = useState("");
  const [bottom, setBottom] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const pending = useRef(false);
  const apply = (value: ShortcutStatus) => {
    setStatus(value);
    setPalette(value.palette);
    setBottom(value.bottomTerminal ?? "");
  };
  // biome-ignore lint/correctness/useExhaustiveDependencies: attempt explicitly reloads native state.
  useEffect(() => {
    let cancelled = false;
    setError(null);
    void shortcutStatus()
      .then((value) => {
        if (!cancelled) apply(value);
      })
      .catch(() => {
        if (!cancelled) setError("전역 단축키 상태를 확인하지 못했습니다.");
      });
    return () => {
      cancelled = true;
    };
  }, [attempt]);
  const save = async () => {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      const value = await configureShortcuts(palette.trim(), bottom.trim() || null);
      if (value.paletteError || value.bottomTerminalError) setStatus(value);
      else apply(value);
    } catch {
      setError("단축키를 저장하지 못했습니다. Ctrl·Alt·Shift 조합과 다른 프로그램의 사용 여부를 확인하세요.");
    } finally {
      pending.current = false;
      setBusy(false);
    }
  };
  return (
    <section aria-label="전역 단축키">
      <h3>전역 단축키</h3>
      {status && (
        <>
          <p>
            <label>
              명령 팔레트 전역 단축키{" "}
              <input value={palette} disabled={busy} onChange={(event) => setPalette(event.target.value)} />
            </label>
          </p>
          {status.paletteError && (
            <p role="alert" style={{ color: "var(--danger)" }}>
              명령 팔레트: {status.paletteError}
            </p>
          )}
          <p>
            <label>
              하단 터미널 전역 단축키{" "}
              <input
                value={bottom}
                disabled={busy}
                placeholder="사용 안 함"
                onChange={(event) => setBottom(event.target.value)}
              />
            </label>
          </p>
          {status.bottomTerminalError && (
            <p role="alert" style={{ color: "var(--danger)" }}>
              하단 터미널: {status.bottomTerminalError}
            </p>
          )}
          <p>Ctrl·Alt·Shift 중 하나를 포함하세요. 하단 터미널은 비워 두면 사용하지 않습니다.</p>
          <Button disabled={busy} onClick={() => void save()}>
            전역 단축키 저장
          </Button>
        </>
      )}
      {error && (
        <p role="alert" style={{ color: "var(--danger)" }}>
          {error}
          {!status && <Button onClick={() => setAttempt((value) => value + 1)}>다시 시도</Button>}
        </p>
      )}
    </section>
  );
}
