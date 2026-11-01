import { invoke } from "@tauri-apps/api/core";
import { useRef, useState } from "react";
import { useConnection } from "../rpc/connection";
import { Banner, Button } from "../ui";

/** 01-design §6.5: the banner offers [다시 시도] after this many failed attempts. */
const ATTEMPTS = 5;
const hhmm = (ms?: number) =>
  ms ? new Date(ms).toLocaleTimeString("ko-KR", { hour: "2-digit", minute: "2-digit" }) : "—";

export function ConnectionBanner() {
  const { state, lastReadyMs, failedAttempts } = useConnection();
  if (state === "versionMismatch")
    return <Banner tone="danger">앱과 WSL 쪽 devbox의 버전이 다릅니다. 두 버전을 맞춘 뒤 앱을 다시 시작하세요.</Banner>;
  if (state === "ready") return null;
  // Mounted until the client is ready again, which is after a Welcome, once every subscription
  // is restored. The Welcome itself resets the count, and `Reconnecting` forgets its retry then.
  return <Reconnecting first={state === "connecting"} failed={failedAttempts} lastReadyMs={lastReadyMs} />;
}

/**
 * While the native gate is ready, the bridge accepts frames even if the daemon behind it hangs;
 * only the missing Welcome shows it. [다시 시도] is `gate_retry`, which forces a daemon restart.
 */
function Reconnecting({ first, failed, lastReadyMs }: { first: boolean; failed: number; lastReadyMs?: number }) {
  // The restart's own attempts count from the retry, so the offer returns only if they fail too.
  const [retriedAt, setRetriedAt] = useState(0);
  // Only a Welcome lowers the count. When that Welcome does not reach ready, this stays mounted,
  // so the retry is forgotten here and the next failures count from zero.
  const baseline = failed < retriedAt ? 0 : retriedAt;
  if (baseline !== retriedAt) setRetriedAt(baseline);
  const [pending, setPending] = useState(false);
  const [refused, setRefused] = useState(false);
  const running = useRef(false);
  const updated = first ? "" : ` · 마지막 갱신 ${hhmm(lastReadyMs)}`;
  const counted = failed - baseline;
  if (!("__TAURI_INTERNALS__" in window) || counted < ATTEMPTS)
    return first ? null : <Banner tone="warn">WSL 쪽 devbox에 다시 연결하고 있습니다{updated}</Banner>;
  const retry = () => {
    if (running.current) return;
    running.current = true;
    setPending(true);
    setRefused(false);
    void invoke("gate_retry", { distro: null })
      .then(
        () => setRetriedAt(failed),
        () => setRefused(true),
      )
      .finally(() => {
        running.current = false;
        setPending(false);
      });
  };
  return (
    <Banner
      tone="warn"
      actions={
        <Button variant="primary" disabled={pending} onClick={retry}>
          다시 시도
        </Button>
      }
    >
      WSL 쪽 devbox가 응답하지 않습니다. [다시 시도]를 누르면 WSL 쪽 devbox를 다시 시작합니다.{updated}
      {refused && <span style={{ display: "block" }}>다시 시도하지 못했습니다. 잠시 후 다시 시도하세요.</span>}
    </Banner>
  );
}
