import type { RunDef } from "../../rpc/gen/rpc";
import { visibleTrustText as visible } from "./visibleTrustText";

export function RunTrustDefinitions({ definitions }: { definitions: RunDef[] }) {
  return (
    <section aria-label="프로젝트 실행 정의">
      <h3>프로젝트 실행 정의</h3>
      {definitions.length === 0 ? (
        <p>없음</p>
      ) : (
        definitions.map((definition) => (
          <div key={`${definition.kind}:${definition.id}`}>
            <h4>
              {definition.kind === "task" ? "작업" : definition.kind === "service" ? "서비스" : "예약"} ·{" "}
              {visible(definition.id)}
            </h4>
            {definition.kind === "schedule" ? (
              <dl>
                <dt>실행할 작업</dt>
                <dd>{visible(definition.task)}</dd>
                <dt>예약 시각</dt>
                <dd>{visible(definition.cron)}</dd>
                <dt>시간대</dt>
                <dd>{visible(definition.tz ?? "시스템 시간대")}</dd>
                <dt>놓친 예약</dt>
                <dd>{definition.missed === "runOnce" ? "한 번 실행" : "건너뛰기"}</dd>
              </dl>
            ) : (
              <>
                <pre>{visible(definition.command)}</pre>
                <dl>
                  <dt>실행 대상</dt>
                  <dd>
                    {definition.target === "windows"
                      ? "Windows에서 실행 · 자식 프로세스 종료는 보장하지 않음"
                      : "Linux"}
                  </dd>
                  <dt>시작 폴더</dt>
                  <dd>{visible(definition.cwd ?? "프로젝트 기준 폴더")}</dd>
                  <dt>작업 폴더</dt>
                  <dd>{visible(definition.worktree ?? "기준 체크아웃")}</dd>
                  <dt>의존 순서</dt>
                  <dd>{definition.dependsOn.length ? definition.dependsOn.map(visible).join(" → ") : "없음"}</dd>
                  <dt>환경 변수</dt>
                  <dd>
                    {Object.entries(definition.env).length
                      ? Object.entries(definition.env).map(([name, value]) => (
                          <pre key={name}>
                            {visible(name)}={visible(value)}
                          </pre>
                        ))
                      : "없음"}
                  </dd>
                  <dt>비밀 이름</dt>
                  <dd>{definition.secrets.length ? definition.secrets.map(visible).join(", ") : "없음"}</dd>
                </dl>
                {definition.kind === "service" && (
                  <dl>
                    <dt>재시작</dt>
                    <dd>
                      {definition.restart === "always"
                        ? "항상"
                        : definition.restart === "onFailure"
                          ? "실패할 때"
                          : "재시작 안 함"}
                      {" · "}
                      대기 {definition.restartSec}초
                    </dd>
                    <dt>시작 횟수 제한</dt>
                    <dd>
                      {definition.startLimit.intervalSec}초 동안 {definition.startLimit.burst}회
                    </dd>
                    <dt>준비 확인 포트</dt>
                    <dd>{definition.portsHint.length ? definition.portsHint.join(", ") : "없음"}</dd>
                    <dt>준비 대기</dt>
                    <dd>{definition.readyTimeoutSec}초</dd>
                  </dl>
                )}
              </>
            )}
          </div>
        ))
      )}
    </section>
  );
}
