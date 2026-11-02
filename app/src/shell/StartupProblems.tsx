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
  activityRules: "개인정보 규칙을 적용하지 못했습니다",
} as const;

/** Sections whose failure policy is the defaults, so nothing is turned off (docs/config.md 오류 동작). */
const DEFAULTED = new Set(["projects", "pc", "terminal", "secrets"]);

/** What the failure policy did to a disabled section. `[agents]` loses only cleanup after close. */
function disabledEffect(section: string) {
  if (section === "agents") return "닫은 작업의 자동 정리를 껐습니다. 도구 프로필은 그대로 씁니다.";
  return DEFAULTED.has(section) ? "이 항목은 기본값으로 실행 중입니다." : "이 기능을 껐습니다.";
}

function Guidance({ problem }: { problem: StartupProblem }) {
  switch (problem.kind) {
    case "configFile":
      return (
        <p style={{ margin: 0 }}>
          <code>{problem.file}</code>에 문법 오류가 있어 기본값으로 실행 중입니다. 노트·검색·활동 기록·Windows 사본·MCP
          쓰기 도구, 사용자 정의 도구 프로필과 닫은 작업의 자동 정리가 꺼져 있습니다. 파일을 고친 뒤 [다시 설정]을
          누르세요.
        </p>
      );
    case "configSection":
      // `ignored` (an unknown section, [windows] in config.toml or one profile entry) turns nothing
      // off. Otherwise the copy follows the section's failure policy.
      return problem.effect === "ignored" ? (
        <p style={{ margin: 0 }}>
          <code>{problem.file}</code>의 [{problem.section}] 항목에서 잘못된 부분만 무시했습니다. 나머지 설정은 그대로
          동작합니다. 고치거나 지운 뒤 [다시 설정]을 누르세요.
        </p>
      ) : (
        <p style={{ margin: 0 }}>
          <code>{problem.file}</code>의 [{problem.section}] 항목에 오류가 있어 {disabledEffect(problem.section)} 고친 뒤
          [다시 설정]을 누르세요.
        </p>
      );
    case "agentsReconcile":
      return (
        <p style={{ margin: 0 }}>
          터미널 서버(tmux)에 연결하지 못했습니다. 자동으로 다시 시도합니다. 계속되면 설정 → 앱 진단을 확인하세요.
        </p>
      );
    case "activityRules":
      // 설정 → 활동 saves to config.toml, and a rule in config.local.toml wins on the next start.
      return (
        <p style={{ margin: 0 }}>
          <code>{problem.file}</code>의 [activity]에 맞지 않는 개인정보 규칙이 있어 활동 기록을 껐습니다.{" "}
          {problem.local
            ? "이 파일의 규칙을 고친 뒤 [다시 설정]을 누르세요."
            : "설정 → 활동에서 규칙을 고쳐 저장하세요."}
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
      {maintenance.detail}
      {copied && <p style={{ margin: 0 }}>{copied}</p>}
      {maintenance.dialog}
    </Banner>
  );
}
