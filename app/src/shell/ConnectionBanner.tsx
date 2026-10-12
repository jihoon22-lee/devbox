import { useConnection } from "../rpc/connection";
import { Banner } from "../ui";

const hhmm = (ms?: number) =>
  ms ? new Date(ms).toLocaleTimeString("ko-KR", { hour: "2-digit", minute: "2-digit" }) : "—";

export function ConnectionBanner() {
  const { state, lastReadyMs } = useConnection();
  if (state === "reconnecting")
    return <Banner tone="warn">데몬에 다시 연결하고 있습니다 · 마지막 갱신 {hhmm(lastReadyMs)}</Banner>;
  if (state === "versionMismatch")
    return <Banner tone="danger">앱과 데몬 버전이 다릅니다. 앱과 데몬 버전을 맞춘 뒤 다시 시작하세요.</Banner>;
  return null;
}
