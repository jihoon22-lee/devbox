import { useRef, useState } from "react";
import { RpcError } from "../../rpc/client";
import { messageFor } from "../../rpc/messages";
import { useRpcMutation, useRpcQuery } from "../../rpc/query";
import { Button, StatusDot } from "../../ui";
import { useCurrentProject } from "../projects/store";
import { AgentCleanup } from "./AgentCleanup";
import { QueryResult } from "./QueryResult";
export function AgentsSettings() {
  const profiles = useRpcQuery("agents.profiles", {});
  const doctor = useRpcQuery("system.doctor", {});
  const info = useRpcQuery("system.info", {});
  const refresh = useRpcMutation("system.refresh_env");
  const project = useCurrentProject((state) => state.id);
  const [notice, setNotice] = useState("");
  const refreshing = useRef(false);
  const reload = async () => {
    if (refreshing.current) return;
    refreshing.current = true;
    setNotice("");
    try {
      const result = await refresh.mutateAsync({});
      setNotice(
        result.restarting
          ? "데몬을 다시 시작합니다. 다시 연결되면 환경을 확인합니다."
          : "셸 환경 파일을 다시 읽었습니다. 현재 데몬에는 다음 시작부터 적용됩니다.",
      );
      if (!result.restarting) void doctor.refetch();
    } catch (error) {
      const failure = error instanceof RpcError ? error : new RpcError("internal");
      const message = messageFor(failure.code, failure.detail);
      setNotice(
        `${message.title} ${message.action ?? ""}${failure.diagnosticId ? ` · 진단 ${failure.diagnosticId}` : ""}`,
      );
    } finally {
      refreshing.current = false;
    }
  };
  const copy = async () => {
    if (!info.data?.configPath) return;
    try {
      await navigator.clipboard.writeText(info.data.configPath);
      setNotice("설정 파일 위치를 복사했습니다");
    } catch {
      setNotice("설정 파일 위치를 복사하지 못했습니다");
    }
  };
  return (
    <>
      <section aria-label="도구 프로필">
        <h3>도구 프로필</h3>
        <QueryResult pending={profiles.isPending} error={profiles.error} retry={() => void profiles.refetch()}>
          <ul>
            {profiles.data?.items.map((profile) => (
              <li key={profile.id}>
                {profile.label} · <code>{profile.command}</code>
              </li>
            ))}
          </ul>
        </QueryResult>
        <p>사용자 정의 도구 프로필은 config.toml에서 편집합니다. 변경한 뒤 데몬을 다시 시작하세요.</p>
        <QueryResult pending={info.isPending} error={info.error} retry={() => void info.refetch()}>
          <p>
            <code>{info.data?.configPath}</code>
          </p>
          <Button disabled={!info.data?.configPath} onClick={() => void copy()}>
            설정 파일 위치 복사
          </Button>
        </QueryResult>
      </section>
      <section aria-label="도구와 상태 알림 점검">
        <h3>도구와 상태 알림 점검</h3>
        <QueryResult pending={doctor.isPending} error={doctor.error} retry={() => void doctor.refetch()}>
          <ul>
            {doctor.data?.checks
              .filter((check) => check.id.startsWith("tool.") || check.id.startsWith("hook."))
              .map((check) => (
                <li key={check.id}>
                  <StatusDot
                    tone={check.status === "ok" ? "ok" : check.status === "warn" ? "warn" : "danger"}
                    label={check.status === "ok" ? "정상" : check.status === "warn" ? "주의" : "실패"}
                  />{" "}
                  {check.message} {check.fix && <span>{check.fix}</span>}
                </li>
              ))}
          </ul>
        </QueryResult>
        <Button disabled={refresh.isPending} onClick={() => void reload()}>
          셸 환경 다시 읽기
        </Button>
        {notice && <p role="status">{notice}</p>}
      </section>
      {project ? <AgentCleanup key={project} projectId={project} /> : <p>정리 후보를 보려면 프로젝트를 선택하세요.</p>}
    </>
  );
}
