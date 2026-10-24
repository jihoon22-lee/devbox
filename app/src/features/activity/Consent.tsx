import { Button } from "../../ui";
import { type ActivityControl, useNativeActivity } from "./nativeState";

export type { ActivityState } from "./nativeState";

export function ActivityConsent() {
  return <ActivityControls control={useNativeActivity()} />;
}
/** Only native consent/pause state decides these buttons; list responses never do. */
export function ActivityControls({ control }: { control: ActivityControl }) {
  const { state, error, busy, change, retry } = control;
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
          {!state && <Button onClick={retry}>다시 시도</Button>}
        </p>
      )}
    </section>
  );
}
