import type { PortInfo, ProcessInfo } from "../../rpc/gen/rpc";
import { openExternal } from "../../shell/external";
import { Button, useToasts } from "../../ui";
import { type ProcessTarget, TerminateButton } from "./ProcessTermination";

type Actions = { busy: boolean; request: (target: ProcessTarget) => void };
/** Use loopback for wildcard bindings; only numeric host addresses from the port snapshot. */
export function portBrowserUrl(port: Pick<PortInfo, "proto" | "addr" | "port">): string | undefined {
  if (port.proto !== "tcp" || !Number.isInteger(port.port) || port.port < 1 || port.port > 65535) return;
  const addr = port.addr === "0.0.0.0" ? "127.0.0.1" : port.addr === "::" ? "::1" : port.addr;
  if (addr.includes(":")) {
    if (!/^[0-9a-fA-F:.]+$/.test(addr)) return;
  } else if (!/^\d{1,3}(\.\d{1,3}){3}$/.test(addr) || addr.split(".").some((part) => Number(part) > 255)) return;
  try {
    return new URL(`http://${addr.includes(":") ? `[${addr}]` : addr}:${port.port}`).href.replace(/\/$/, "");
  } catch {
    return;
  }
}
export function PortsTable({ ports, actions }: { ports: PortInfo[]; actions: Actions }) {
  return (
    <table>
      <thead>
        <tr>
          <th>포트</th>
          <th>프로토콜</th>
          <th>주소</th>
          <th>프로세스</th>
          <th>PID</th>
          <th>실행</th>
          <th>동작</th>
        </tr>
      </thead>
      <tbody>
        {ports.map((port, index) => {
          const url = portBrowserUrl(port);
          const target =
            port.pid && port.pid > 1 && port.startTimeMs != null && !port.containerId
              ? { pid: port.pid, name: port.process ?? "이름 없음", startTimeMs: port.startTimeMs }
              : undefined;
          return (
            <tr key={`${port.proto}:${port.addr}:${port.port}:${port.pid}:${index}`}>
              <td>{port.port}</td>
              <td>{port.proto}</td>
              <td>{port.addr}</td>
              <td>{port.process ?? "—"}</td>
              <td>{port.pid ?? "—"}</td>
              <td>{port.runId ?? port.containerId ?? "—"}</td>
              <td>
                <Button
                  size="sm"
                  disabled={!url}
                  aria-label={`포트 ${port.port} 브라우저로 열기`}
                  onClick={() =>
                    url &&
                    void openExternal(url).catch(() =>
                      useToasts.getState().push({ tone: "danger", title: "브라우저를 열지 못했습니다" }),
                    )
                  }
                >
                  브라우저로 열기
                </Button>
                <TerminateButton target={target} busy={actions.busy} onRequest={actions.request} />
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
export function ProcessesTable({ processes, actions }: { processes: ProcessInfo[]; actions: Actions }) {
  return (
    <table>
      <thead>
        <tr>
          <th>프로세스</th>
          <th>PID</th>
          <th>작업 폴더</th>
          <th>프로젝트</th>
          <th>CPU</th>
          <th>메모리</th>
          <th>동작</th>
        </tr>
      </thead>
      <tbody>
        {processes.map((process) => (
          <tr key={`${process.pid}:${process.startTimeMs}`}>
            <td>{process.name}</td>
            <td>{process.pid}</td>
            <td>{process.cwd ?? "—"}</td>
            <td>{process.projectId ?? "—"}</td>
            <td>{process.cpuPercent.toFixed(1)}%</td>
            <td>{(process.rssBytes / 1048576).toFixed(1)} MiB</td>
            <td>
              <TerminateButton
                target={process.pid > 1 ? process : undefined}
                busy={actions.busy}
                onRequest={actions.request}
              />
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
