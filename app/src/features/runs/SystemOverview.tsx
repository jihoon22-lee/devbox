import { useNavigate } from "@tanstack/react-router";
import { Button, ErrorState } from "../../ui";
import { gigabytes, memoryLabel, useSystemMetrics } from "./systemMetrics";

export function SystemOverview({ view }: { view: "system" | "ports" }) {
  const navigate = useNavigate();
  const { system, ports } = useSystemMetrics(view);
  const query = view === "system" ? system : ports;
  return (
    <section
      style={{ padding: "var(--space-4)", overflow: "auto" }}
      aria-label={view === "system" ? "WSL 리소스" : "포트 목록"}
    >
      <Button onClick={() => void navigate({ to: "/runs", search: {} })}>실행 목록</Button>
      <h2>{view === "system" ? "WSL 리소스" : "포트 목록"}</h2>
      {query.isError ? (
        <ErrorState
          title={view === "system" ? "WSL 리소스를 불러오지 못했습니다" : "포트를 불러오지 못했습니다"}
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
      ) : ports.data?.length ? (
        <table>
          <thead>
            <tr>
              <th>포트</th>
              <th>프로토콜</th>
              <th>주소</th>
              <th>프로세스</th>
              <th>PID</th>
              <th>실행</th>
            </tr>
          </thead>
          <tbody>
            {ports.data.map((port, index) => (
              <tr key={`${port.proto}:${port.addr}:${port.port}:${port.pid}:${index}`}>
                <td>{port.port}</td>
                <td>{port.proto}</td>
                <td>{port.addr}</td>
                <td>{port.process ?? "—"}</td>
                <td>{port.pid ?? "—"}</td>
                <td>{port.runId ?? port.containerId ?? "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : (
        <p>열린 포트가 없습니다.</p>
      )}
    </section>
  );
}
