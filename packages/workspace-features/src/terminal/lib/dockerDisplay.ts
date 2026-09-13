export type DockerStateKey =
  | "running"
  | "paused"
  | "restarting"
  | "removing"
  | "exited"
  | "created"
  | "dead"
  | "unknown";

export interface DockerDisplayState {
  key: DockerStateKey;
  label: string;
  running: boolean;
}

const STATES: Record<DockerStateKey, DockerDisplayState> = {
  running: { key: "running", label: "실행 중", running: true },
  paused: { key: "paused", label: "일시 중지", running: false },
  restarting: { key: "restarting", label: "재시작 중", running: false },
  removing: { key: "removing", label: "제거 중", running: false },
  exited: { key: "exited", label: "종료됨", running: false },
  created: { key: "created", label: "생성됨", running: false },
  dead: { key: "dead", label: "중단됨", running: false },
  unknown: { key: "unknown", label: "알 수 없음", running: false },
};

/** Docker의 사람이 읽는 STATUS 원문은 보존하고, 좁은 summary용 상태만 분류한다. */
export function dockerDisplayState(status: string): DockerDisplayState {
  const normalized = status.trim().toLowerCase();
  if (normalized.startsWith("paused") || normalized.includes("(paused)")) {
    return STATES.paused;
  }
  if (normalized === "up" || normalized.startsWith("up ")) {
    return STATES.running;
  }
  if (normalized.startsWith("restarting")) return STATES.restarting;
  if (normalized.startsWith("removal") || normalized.startsWith("removing")) {
    return STATES.removing;
  }
  if (normalized.startsWith("exited")) return STATES.exited;
  if (normalized.startsWith("created")) return STATES.created;
  if (normalized.startsWith("dead")) return STATES.dead;
  return STATES.unknown;
}

function compactPort(port: string): string {
  const [published, target] = port.split("->", 2).map((part) => part.trim());
  if (!target) return published;

  // IPv4(0.0.0.0:8080), bracketed IPv6([::]:8080), Docker's compact IPv6
  // form(:::8080) all place the published port after the final colon.
  const colon = published.lastIndexOf(":");
  const hostPort = colon >= 0 ? published.slice(colon + 1) : published;
  return hostPort ? `${hostPort}→${target}` : target;
}

/** 최대 두 개의 고유 port mapping만 보여 주고 나머지는 detail 원문에 남긴다. */
export function compactDockerPorts(ports: string): string {
  const unique = Array.from(
    new Set(
      ports
        .split(",")
        .map((port) => port.trim())
        .filter(Boolean)
        .map(compactPort),
    ),
  );

  if (unique.length === 0) return "포트 없음";
  const visible = unique.slice(0, 2).join(", ");
  return unique.length > 2 ? `${visible} +${unique.length - 2}` : visible;
}
