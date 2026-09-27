import { useEffect, useRef, useState } from "react";
import { toolsCall } from "@devbox/control-center-features/calls";
import type { AgentAutostartStatus } from "@devbox/control-center-features/generated/AgentAutostartStatus";

export default function AgentAutostart() {
  const [status, setStatus] = useState<AgentAutostartStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const serial = useRef(0);
  const saving = useRef(false);
  useEffect(() => {
    let active = true;
    const refresh = () => {
      if (saving.current) return;
      const request = ++serial.current;
      void toolsCall("autostart_status", {}).then(
        (value) => {
          if (active && request === serial.current) {
            setStatus(value);
            setError(null);
          }
        },
        (error: unknown) => {
          if (active && request === serial.current)
            setError(error instanceof Error ? error.message : "자동 시작 설정을 확인할 수 없습니다.");
        },
      );
    };
    refresh();
    window.addEventListener("focus", refresh);
    return () => {
      active = false;
      window.removeEventListener("focus", refresh);
    };
  }, []);
  async function change(enabled: boolean) {
    const request = ++serial.current;
    saving.current = true;
    setBusy(true);
    setError(null);
    try {
      const value = await toolsCall("set_autostart", { enabled });
      if (request === serial.current) setStatus(value);
    } catch (error) {
      if (request === serial.current)
        setError(error instanceof Error ? error.message : "자동 시작 설정을 저장할 수 없습니다.");
    } finally {
      saving.current = false;
      setBusy(false);
    }
  }
  return (
    <section className="panel" aria-label="백그라운드 서비스">
      <h2>백그라운드 서비스</h2>
      {status?.supported ? (
        <label>
          <input
            type="checkbox"
            checked={status.enabled}
            disabled={busy}
            onChange={(event) => {
              void change(event.currentTarget.checked);
            }}
          />
          로그인할 때 백그라운드 서비스 시작
        </label>
      ) : (
        <p>
          {status ? "설치된 Windows Suite에서 자동 시작을 설정할 수 있습니다." : "자동 시작 설정을 확인하는 중입니다."}
        </p>
      )}
      <p>제품 창을 열지 않고 예약 작업·웹훅·검색 색인을 시작합니다. 활동 기록은 별도의 수집 동의가 필요합니다.</p>
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
