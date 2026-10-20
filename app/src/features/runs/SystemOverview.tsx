import { useRpcQuery } from "../../rpc/query";
import { usePageVisible } from "../../shell/visibility";
import { Button, ErrorState } from "../../ui";
import { ObservationNavigation, type ObservationView, observationLabels } from "./ObservationNavigation";
import { PortsTable, ProcessesTable } from "./ObservationTables";
import { useProcessTermination } from "./ProcessTermination";
import { gigabytes, memoryLabel, useSystemMetrics } from "./systemMetrics";

export function SystemOverview({ view, projectId }: { view: ObservationView; projectId?: string }) {
  const { system, ports } = useSystemMetrics(view);
  const visible = usePageVisible();
  const processes = useRpcQuery(
    "observe.processes",
    { projectId: projectId ?? null },
    {
      enabled: visible && view === "processes",
      refetchInterval: visible ? 3000 : false,
      refetchIntervalInBackground: true,
      staleTime: 0,
    },
  );
  const actions = useProcessTermination();
  const query = view === "system" ? system : view === "ports" ? ports : processes;
  return (
    <section style={{ padding: "var(--space-4)", overflow: "auto" }} aria-label={observationLabels[view]}>
      <ObservationNavigation view={view} />
      <h2>{observationLabels[view]}</h2>
      {actions.feedback}
      {view === "processes" && (
        <p>
          {projectId
            ? "현재 프로젝트 관련 프로세스를 먼저 표시하고, 나머지는 CPU 사용량 상위 50개를 표시합니다."
            : "CPU 사용량 상위 50개 프로세스를 표시합니다."}
        </p>
      )}
      {query.isError ? (
        <ErrorState
          title={
            view === "system"
              ? "WSL 리소스를 불러오지 못했습니다"
              : view === "ports"
                ? "포트를 불러오지 못했습니다"
                : "프로세스를 불러오지 못했습니다"
          }
          action={<Button onClick={() => void query.refetch()}>다시 시도</Button>}
        />
      ) : query.isPending ? (
        <p role="status">불러오는 중…</p>
      ) : view === "system" && system.data ? (
        <dl>
          <dt>메모리 사용</dt>
          <dd>{memoryLabel(system.data)}</dd>
          <dt>스왑 사용</dt>
          <dd>
            {gigabytes(system.data.swapUsedBytes)}/{gigabytes(system.data.swapTotalBytes)}GB
          </dd>
          <dt>CPU 대기 비율 (최근 10초)</dt>
          <dd>{system.data.cpuPressure?.some ? `${system.data.cpuPressure.some.avg10}%` : "정보 없음"}</dd>
          <dt>메모리 대기 비율 (최근 10초)</dt>
          <dd>{system.data.memoryPressure?.some ? `${system.data.memoryPressure.some.avg10}%` : "정보 없음"}</dd>
          <dt>입출력 대기 비율 (최근 10초)</dt>
          <dd>{system.data.ioPressure?.some ? `${system.data.ioPressure.some.avg10}%` : "정보 없음"}</dd>
          <dt>홈 디스크 사용</dt>
          <dd>
            {system.data.homeFilesystem
              ? `${gigabytes(system.data.homeFilesystem.usedBytes)}/${gigabytes(system.data.homeFilesystem.totalBytes)}GB`
              : "정보 없음"}
          </dd>
        </dl>
      ) : view === "processes" ? (
        processes.data?.length ? (
          <ProcessesTable processes={processes.data} actions={actions} />
        ) : (
          <p>프로세스가 없습니다.</p>
        )
      ) : ports.data?.length ? (
        <PortsTable ports={ports.data} actions={actions} />
      ) : (
        <p>열린 포트가 없습니다.</p>
      )}
    </section>
  );
}
