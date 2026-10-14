import { useRef } from "react";
import type { Methods } from "../../rpc/gen/rpc";
import { Button, Dialog } from "../../ui";
import s from "./ProjectTrust.module.css";
import { visibleTrustText } from "./visibleTrustText";

type Preview = Methods["projects.trust_preview"]["result"];
export function ProjectTrustDialog({
  preview,
  pending,
  notice,
  onCancel,
  onApprove,
  onRetry,
}: {
  preview?: Preview;
  pending: boolean;
  notice: string;
  onCancel: () => void;
  onApprove: () => void;
  onRetry: () => void;
}) {
  const cancel = useRef<HTMLButtonElement>(null);
  const config = preview?.config;
  return (
    <Dialog
      open
      title="프로젝트 명령 실행 승인"
      description="프로젝트 설정에 담긴 명령을 실행하기 전에 경로와 전체 내용을 확인하세요."
      width={680}
      onOpenChange={(open) => !open && onCancel()}
      onOpenAutoFocus={(event) => {
        event.preventDefault();
        cancel.current?.focus();
      }}
      footer={
        <>
          <Button ref={cancel} onClick={onCancel}>
            승인 취소
          </Button>
          {!preview && !pending && <Button onClick={onRetry}>다시 확인</Button>}
          <Button variant="primary" disabled={pending || !preview?.digest || !config} onClick={onApprove}>
            명령 실행 승인
          </Button>
        </>
      }
    >
      {notice && <p role="alert">{notice}</p>}
      {pending && <p role="status">명령 설정을 확인하는 중…</p>}
      {preview && config && (
        <div className={s.preview}>
          <p>
            {
              "방향 제어 문자 등 보이지 않는 제어 문자는 \u005cuXXXX로, 역슬래시는 \u005c\u005c로 표시합니다. 실행할 원문은 바뀌지 않습니다."
            }
          </p>
          <dl className={s.paths}>
            <dt>프로젝트</dt>
            <dd>{visibleTrustText(preview.projectName)}</dd>
            <dt>프로젝트 경로</dt>
            <dd>{visibleTrustText(preview.canonicalPath)}</dd>
            <dt>설정 파일</dt>
            <dd>{visibleTrustText(preview.configPath)}</dd>
          </dl>
          <section aria-label="실행 위치">
            <h3>실행 위치</h3>
            <ul>
              {preview.executionDirectories.map((path) => (
                <li key={path}>{visibleTrustText(path)}</li>
              ))}
            </ul>
          </section>
          <p>준비·테스트·정리 명령은 해당 에이전트 작업의 실행 위치에서 실행합니다.</p>
          <Commands label="작업 폴더 준비 명령" commands={config.agent.setup} />
          <Commands label="테스트 명령" commands={config.agent.test ? [config.agent.test] : []} />
          <Commands label="작업 폴더 정리 명령" commands={config.agent.teardown} />
          <section aria-label="시작 구성 터미널">
            <h3>시작 구성 터미널</h3>
            {config.launch.terminals.length ? (
              config.launch.terminals.map((terminal, index) => (
                <div key={`${terminal.name}:${index}`}>
                  <p>
                    {visibleTrustText(terminal.name)} · 실행 위치: {visibleTrustText(preview.canonicalPath)}
                  </p>
                  <pre>{visibleTrustText(terminal.command ?? "기본 셸")}</pre>
                </div>
              ))
            ) : (
              <p>없음</p>
            )}
          </section>
          <section aria-label="시작 구성 에이전트">
            <h3>시작 구성 에이전트</h3>
            {config.launch.agents.length ? (
              config.launch.agents.map((agent, index) => (
                <div key={`${agent.tool}:${index}`}>
                  <p>
                    도구: {visibleTrustText(agent.tool)} ·{" "}
                    {agent.worktree ? "작업 폴더 만들기" : "현재 체크아웃에서 실행"}
                  </p>
                  <p>
                    실행 위치:{" "}
                    {agent.worktree
                      ? `${visibleTrustText(preview.canonicalPath)}-agents/<새 작업 폴더> (생성할 때 이름 확정)`
                      : visibleTrustText(preview.canonicalPath)}
                  </p>
                  <pre>{visibleTrustText(agent.prompt ?? "지시문 없음")}</pre>
                </div>
              ))
            ) : (
              <p>없음</p>
            )}
          </section>
          <p>승인은 프로젝트 경로와 설정 내용이 유지되는 동안 저장됩니다. 경로나 설정이 바뀌면 다시 승인해야 합니다.</p>
          <p>명령은 사용자 권한으로 실행됩니다. 명령이 부르는 외부 스크립트와 작업 폴더는 격리되지 않습니다.</p>
        </div>
      )}
    </Dialog>
  );
}
function Commands({ label, commands }: { label: string; commands: string[] }) {
  return (
    <section aria-label={label}>
      <h3>{label}</h3>
      {commands.length ? (
        commands.map((command, index) => <pre key={`${index}:${command}`}>{visibleTrustText(command)}</pre>)
      ) : (
        <p>없음</p>
      )}
    </section>
  );
}
