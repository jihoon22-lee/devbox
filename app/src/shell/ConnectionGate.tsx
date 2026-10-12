// app/src/shell/ConnectionGate.tsx
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { Banner, Button, EmptyState } from "../ui";

export interface GateStatus {
  instance: "dev" | "prod";
  revision: string;
  state:
    | "checking"
    | "noWsl"
    | "chooseDistro"
    | "wsl1"
    | "systemdOff"
    | "deploying"
    | "setupFailed"
    | "connecting"
    | "ready"
    | "bridgeFailed";
  detail?: string | null;
  distros?: { name: string; version: number; default: boolean }[] | null;
}

function CompactGate({ title, body, action }: { title: string; body?: string; action?: ReactNode }) {
  return (
    <Banner tone="warn" actions={action}>
      <strong>{title}</strong>
      {body && <span style={{ display: "block" }}>{body}</span>}
    </Banner>
  );
}
export function GateView({
  status,
  onRetry,
  compact = false,
  busy = false,
}: {
  status: GateStatus;
  onRetry: (distro?: string) => void;
  compact?: boolean;
  busy?: boolean;
}) {
  const View = compact ? CompactGate : EmptyState;
  const retry = (
    <Button variant="primary" disabled={busy} onClick={() => onRetry()}>
      다시 시도
    </Button>
  );
  switch (status.state) {
    case "checking":
    case "connecting":
      return <View title="WSL의 devbox에 연결하고 있습니다" />;
    case "deploying":
      return <View title="devbox를 WSL에 설치하고 있습니다" body="처음 실행하거나 업데이트한 뒤 한 번만 합니다." />;
    case "noWsl":
      return (
        <View
          title="WSL이 필요합니다"
          body="관리자 PowerShell에서 wsl --install 을 실행하고 PC를 다시 시작한 뒤 devbox를 여세요."
          action={retry}
        />
      );
    case "wsl1":
      return (
        <View
          title={`${status.detail ?? ""}은(는) WSL1입니다`}
          body="WSL2만 지원합니다. PowerShell에서 wsl --set-version <배포판> 2 를 실행하세요."
          action={retry}
        />
      );
    case "systemdOff":
      return (
        <View
          title="WSL에서 systemd를 켜야 합니다"
          body={`${status.detail ?? "배포판"}의 /etc/wsl.conf 에 [boot] systemd=true 를 넣고, PowerShell에서 wsl --shutdown 을 실행하세요. 실행 중인 WSL 프로그램이 모두 종료됩니다.`}
          action={retry}
        />
      );
    case "chooseDistro":
      return (
        <View
          title="devbox가 쓸 WSL 배포판을 고르세요"
          action={
            <div style={{ display: "flex", gap: 8, justifyContent: "center" }}>
              {status.distros?.map((d) => (
                <Button
                  key={d.name}
                  disabled={busy}
                  variant={d.default ? "primary" : "secondary"}
                  onClick={() => onRetry(d.name)}
                >
                  {d.name} 사용
                </Button>
              ))}
            </div>
          }
        />
      );
    case "setupFailed":
      return <View title="WSL 쪽 설치에 실패했습니다" body={status.detail ?? ""} action={retry} />;
    case "bridgeFailed":
      return <View title="데몬에 연결하지 못했습니다" body={status.detail ?? ""} action={retry} />;
    case "ready":
      return null;
  }
}

export function ConnectionGate({ children }: { children: ReactNode | ((instance: "dev" | "prod") => ReactNode) }) {
  const [status, setStatus] = useState<GateStatus | null>(null);
  const [instance, setInstance] = useState<"dev" | "prod" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [retrying, setRetrying] = useState(false);
  const lifetime = useRef(0);
  const revision = useRef(-1n);
  // biome-ignore lint/correctness/useExhaustiveDependencies: an explicit retry restarts initialization.
  useEffect(() => {
    const generation = ++lifetime.current;
    let off: (() => void) | undefined;
    const current = () => lifetime.current === generation;
    const receive = (value: GateStatus) => {
      if (!current() || !/^\d+$/.test(value.revision) || BigInt(value.revision) <= revision.current) return;
      revision.current = BigInt(value.revision);
      setStatus(value);
      setError(null);
      if (value.state === "ready") setInstance(value.instance);
    };
    setError(null);
    setRetrying(false);
    void (async () => {
      try {
        const stop = await listen<GateStatus>("devbox://gate", (event) => receive(event.payload));
        if (!current()) {
          stop();
          return;
        }
        off = stop;
        receive(await invoke<GateStatus>("gate_status"));
      } catch {
        if (current()) setError("연결 상태를 확인하지 못했습니다. 다시 시도하세요.");
      }
    })();
    return () => {
      lifetime.current++;
      off?.();
    };
  }, [attempt]);
  const retry = (distro?: string) => {
    if (retrying) return;
    const generation = lifetime.current;
    setRetrying(true);
    setError(null);
    void invoke("gate_retry", { distro: distro ?? null })
      .then(() => {
        if (generation !== lifetime.current) return;
        setRetrying(false);
        setAttempt((value) => value + 1);
      })
      .catch(() => {
        if (generation !== lifetime.current) return;
        setRetrying(false);
        setError("다시 시도하지 못했습니다. 잠시 후 다시 시도하세요.");
      });
  };
  const failure = error && (
    <div role="alert">
      {error}
      <Button disabled={retrying} onClick={() => retry()}>
        다시 시도
      </Button>
    </div>
  );
  if (instance)
    return (
      <div style={{ display: "flex", flexDirection: "column", height: "100%" }}>
        {failure ??
          (status && !["ready", "connecting", "checking"].includes(status.state) ? (
            <GateView status={status} onRetry={retry} compact busy={retrying} />
          ) : null)}
        <div style={{ flex: 1, minHeight: 0 }}>{typeof children === "function" ? children(instance) : children}</div>
      </div>
    );
  if (failure) return failure;
  return (
    <GateView
      status={status ?? { state: "checking", instance: "dev", revision: "0" }}
      onRetry={retry}
      busy={retrying}
    />
  );
}
