import { useRef, useState } from "react";
import type { Check } from "../../../rpc/gen/rpc";
import { useRpcQuery } from "../../../rpc/query";
import { Button, ConfirmDialog, StatusDot } from "../../../ui";
import { openPopout } from "../../terminal/popout";
import { useCreateSession } from "../../terminal/useSessions";
import { type ExecutablePlan, executeInstallation, installationDetails } from "./installAction";

function CheckRow({
  check,
  busy,
  install,
  manual,
}: {
  check: Check;
  busy: boolean;
  install: (plan: ExecutablePlan) => void;
  manual: (text: string) => void;
}) {
  const query = useRpcQuery("tools.install_command", { id: check.id });
  const plan = query.data;
  const title = plan?.kind === "wsl" ? plan.title : "Docker Desktop 설치";
  return (
    <li style={{ display: "flex", gap: "var(--space-2)", flexWrap: "wrap", marginBottom: "var(--space-2)" }}>
      <StatusDot
        tone={check.status === "ok" ? "ok" : check.status === "warn" ? "warn" : "danger"}
        label={check.status === "ok" ? "정상" : check.status === "warn" ? "주의" : "실패"}
      />
      <span>{check.message}</span>
      {check.fix && <code>{check.fix}</code>}
      {plan && (plan.kind === "wsl" || plan.kind === "windows") && (
        <Button disabled={busy} aria-label={title} onClick={() => install({ ...plan })}>
          설치
        </Button>
      )}
      {plan?.kind === "manual" && (
        <Button disabled={busy} onClick={() => manual(plan.text)}>
          설치 안내
        </Button>
      )}
    </li>
  );
}
export function DoctorList({ checks }: { checks: Check[] }) {
  const create = useCreateSession();
  const [selected, setSelected] = useState<ExecutablePlan | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [sessionId, setSessionId] = useState<string | null>(null);
  const running = useRef(false);
  const confirm = async () => {
    if (!selected || running.current) return;
    running.current = true;
    setBusy(true);
    setSelected(null);
    setNotice("");
    setSessionId(null);
    try {
      const session = await executeInstallation(selected, create);
      if (session) {
        setSessionId(session.id);
        try {
          await openPopout("terminal", session.id);
          setNotice("설치 터미널을 열었습니다");
        } catch {
          setNotice("설치가 시작되었습니다. 터미널을 열어 출력을 확인해 주세요.");
        }
      } else setNotice("Windows 설치 창을 열었습니다");
    } catch {
      setNotice("설치 요청 결과를 확인하지 못했습니다. 중복 실행 전에 터미널과 설치 창을 확인해 주세요.");
    } finally {
      running.current = false;
      setBusy(false);
    }
  };
  const detail = selected ? installationDetails(selected) : null;
  return (
    <section aria-label="환경 점검">
      <h3>환경 점검</h3>
      <ul style={{ listStyle: "none", padding: 0 }}>
        {checks.map((check) => (
          <CheckRow key={check.id} check={check} busy={busy} install={setSelected} manual={setNotice} />
        ))}
      </ul>
      {checks.length === 0 && <p>점검 항목이 없습니다</p>}
      {notice && <p role="status">{notice}</p>}
      {sessionId && (
        <Button
          onClick={() => void openPopout("terminal", sessionId).catch(() => setNotice("터미널을 열지 못했습니다"))}
        >
          설치 터미널 열기
        </Button>
      )}
      <ConfirmDialog
        open={selected !== null}
        title={detail?.title ?? "설치"}
        body={detail?.body ?? ""}
        confirmLabel="실행"
        tone="primary"
        disabled={busy}
        onConfirm={() => void confirm()}
        onCancel={() => setSelected(null)}
      >
        <pre>{detail?.command}</pre>
      </ConfirmDialog>
    </section>
  );
}
