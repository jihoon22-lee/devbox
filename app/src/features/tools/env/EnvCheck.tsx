import { useRef, useState } from "react";
import { useRpcMutation, useRpcQuery } from "../../../rpc/query";
import { Button } from "../../../ui";
import { QueryResult } from "../../settings/QueryResult";
import { DoctorList } from "./DoctorList";
import { NativeMaintenance } from "./NativeMaintenance";
import { SupportBundle } from "./SupportBundle";

function RefreshEnvironment({ refetch }: { refetch: () => void }) {
  const mutation = useRpcMutation("system.refresh_env");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const running = useRef(false);
  const refresh = async () => {
    if (running.current) return;
    running.current = true;
    setBusy(true);
    setNotice("");
    try {
      const result = await mutation.mutateAsync({});
      setNotice(
        result.restarting
          ? "데몬을 다시 시작합니다. 다시 연결되면 환경을 확인합니다."
          : "셸 환경 파일을 다시 읽었습니다. 현재 데몬에는 다음 시작부터 적용됩니다.",
      );
      if (!result.restarting) refetch();
    } catch {
      setNotice("셸 환경을 다시 읽지 못했습니다");
    } finally {
      running.current = false;
      setBusy(false);
    }
  };
  return (
    <>
      <Button disabled={busy} onClick={() => void refresh()}>
        셸 환경 다시 읽기
      </Button>
      {notice && <p role="status">{notice}</p>}
    </>
  );
}
export function EnvCheck() {
  const doctor = useRpcQuery("system.doctor", {});
  const info = useRpcQuery("system.info", {});
  const diagnostics = useRpcQuery("tools.diagnostics", {});
  const failed = doctor.error ? doctor : info.error ? info : diagnostics;
  return (
    <QueryResult
      pending={doctor.isPending || info.isPending || diagnostics.isPending}
      error={failed.error}
      retry={() => void failed.refetch()}
    >
      {doctor.data && info.data && diagnostics.data && (
        <>
          <section aria-label="데몬 상태">
            <h3>데몬 상태</h3>
            <p>
              {info.data.version} · {info.data.instance} · PID {info.data.pid}
            </p>
            <p>
              로그 위치: <code>{diagnostics.data.logDir}</code>
            </p>
          </section>
          <DoctorList checks={doctor.data.checks} />
          <Button onClick={() => void doctor.refetch()}>점검 새로고침</Button>{" "}
          <RefreshEnvironment refetch={() => void doctor.refetch()} />
          <SupportBundle />
          <NativeMaintenance />
        </>
      )}
    </QueryResult>
  );
}
