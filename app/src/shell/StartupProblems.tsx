import { useState } from "react";
import { useWslMaintenance } from "../features/tools/env/NativeMaintenance";
import type { StartupProblem } from "../rpc/gen/rpc";
import { useRpcQuery } from "../rpc/query";
import { Banner, Button } from "../ui";

/** Problems can clear without a restart (the agents retry), and no topic announces it. */
const POLL_MS = 10_000;
const TITLE = {
  configFile: "설정 파일을 읽지 못했습니다",
  configSection: "설정 일부를 읽지 못했습니다",
  agentsReconcile: "에이전트 상태를 확인하지 못했습니다",
} as const;

function Guidance({ problem }: { problem: StartupProblem }) {
  switch (problem.kind) {
    case "configFile":
      return (
        <p style={{ margin: 0 }}>
          <code>{problem.file}</code>에 문법 오류가 있어 기본값으로 실행 중입니다. 노트·검색·Windows 사본과 사용자 정의
          도구 프로필이 꺼져 있습니다. 파일을 고친 뒤 [다시 설정]을 누르세요.
        </p>
      );
    case "configSection":
      return (
        <p style={{ margin: 0 }}>
          <code>{problem.file}</code>의 [{problem.section}] 항목에 오류가 있어 이 기능을 껐습니다. 고친 뒤 [다시 설정]을
          누르세요.
        </p>
      );
    case "agentsReconcile":
      return (
        <p style={{ margin: 0 }}>
          터미널 서버(tmux)에 연결하지 못했습니다. 자동으로 다시 시도합니다. 계속되면 설정 → 앱 진단을 확인하세요.
        </p>
      );
  }
}

/** What the daemon turned off or runs on defaults since it started, and how to recover. */
export function StartupProblems() {
  const [polling, setPolling] = useState(false);
  const info = useRpcQuery("system.info", {}, { refetchInterval: polling ? POLL_MS : false });
  const problems = info.data?.startupProblems ?? [];
  // Adjusts during render (no effect) so the next query options already carry the interval.
  const present = problems.length > 0;
  if (present !== polling) setPolling(present);
  const maintenance = useWslMaintenance();
  const [copied, setCopied] = useState("");
  if (!problems.length) return null;
  const copy = async (file: string) => {
    try {
      await navigator.clipboard.writeText(file);
      setCopied("설정 파일 경로를 복사했습니다");
    } catch {
      setCopied("설정 파일 경로를 복사하지 못했습니다");
    }
  };
  return (
    <Banner tone="warn">
      <ul
        style={{
          margin: 0,
          padding: 0,
          listStyle: "none",
          maxHeight: "40vh",
          overflowY: "auto",
          overflowWrap: "anywhere",
        }}
      >
        {problems.map((problem, index) => (
          <li key={`${index}:${problem.kind}`} style={{ display: "grid", gap: 4, paddingBlock: 4 }}>
            <strong>{TITLE[problem.kind]}</strong>
            <Guidance problem={problem} />
            <code style={{ color: "var(--fg-muted)", whiteSpace: "pre-wrap" }}>{problem.message}</code>
            {problem.kind !== "agentsReconcile" && (
              <span style={{ display: "flex", gap: 8 }}>
                <Button
                  size="sm"
                  disabled={maintenance.busy || !maintenance.native}
                  onClick={() => maintenance.request("redeploy_wsl")}
                >
                  다시 설정
                </Button>
                <Button size="sm" onClick={() => void copy(problem.file)}>
                  경로 복사
                </Button>
              </span>
            )}
          </li>
        ))}
      </ul>
      {maintenance.notice && <p style={{ margin: 0 }}>{maintenance.notice}</p>}
      {copied && <p style={{ margin: 0 }}>{copied}</p>}
      {maintenance.dialog}
    </Banner>
  );
}
