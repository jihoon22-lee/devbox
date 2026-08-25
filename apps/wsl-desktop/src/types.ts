export type Layout = "grid" | "cols" | "rows";
export type MultiplexerKind = "native" | "tmux" | "zellij";

/** 세션 하나. 소속 탭 정보는 갖지 않는다 — Tab.paneIds가 소속의 단일 진실 소스다.
 *
 * `key`와 `sessionId`는 서로 다른 것을 가리킨다(설계 §2.5):
 * - `key`: 팬 자체의 정체성. 프론트가 `makeId`로 생성하며 팬의 수명 동안 절대 바뀌지
 *   않는다. React key(재마운트 방지)와 향후 레이아웃 영속화·멀티플렉서 세션명에 쓴다.
 * - `sessionId`: 지금 이 팬에 붙어 있는 백엔드 PTY 세션의 런타임 id. 백엔드가 발급하며
 *   재시작마다 새로 부여된다. 세션이 아직 붙지 않은 팬을 표현할 수 있도록 null을
 *   허용한다(예: 향후 레이아웃 복원에서 세션 연결 전 상태).
 */
export interface Pane {
  key: string;
  sessionId: string | null;
  distro: string;
  /** OSC 0/2가 보고한 현재 터미널 제목. distro는 값이 없을 때의 fallback이다. */
  title?: string;
  /** 시작 경로로 초기화되고 이후 유효한 OSC 7을 받을 때 현재 경로로 갱신된다. */
  cwd?: string;
  /** 프로필/마지막 레이아웃에서 새 세션에 한 번 전달할 명령. 재연결 시 재실행하지 않는다. */
  startCommand?: string;
  /** 이 런타임 세션이 처음 마운트될 때만 전달할 명령. 영속 정의에는 저장하지 않는다. */
  initialCommand?: string;
  /** 실제로 시작된 모드. 요청한 외부 도구가 없으면 backend가 native로 낮춘다. */
  multiplexer: MultiplexerKind;
}

/** 탭 하나. 항상 팬을 최소 1개 갖는다 (탭 생성은 첫 세션 시작 성공과 함께 일어나고,
 * 마지막 팬이 닫히면 탭도 함께 닫힌다). */
export interface Tab {
  id: string;
  title: string;
  /** OSC 제목을 따라가는 기본 탭(false/undefined)인지 사용자가 직접 이름을 붙였는지. */
  customTitle?: boolean;
  layout: Layout;
  paneIds: string[];
}

export interface DistroInfo {
  name: string;
  version: number;
  default: boolean;
  state: string;
}

export interface ContainerInfo {
  id: string;
  name: string;
  image: string;
  status: string;
  ports: string;
}

/**
 * Inbound cross-app open request (`crates/applink::OpenTarget`/`OpenRequest`).
 * `Option` fields serialize as `null`, never omitted, so every optional field
 * here is typed `| null` rather than `?:`
 * (`docs/superpowers/specs/2026-08-17-app-interop-design.md` §1.2).
 */
export type OpenTarget =
  | { kind: "path"; path: string; line: number | null; column: number | null }
  | { kind: "profile"; id: string }
  | { kind: "workspace"; path: string }
  | { kind: "query"; text: string };

export interface OpenRequest {
  target: OpenTarget;
  from: string | null;
}

export interface WorkspacePaneDefinition {
  key: string;
  distro: string;
  cwd: string | null;
  startCommand: string | null;
  multiplexer: MultiplexerKind;
}

export interface WorkspaceTabDefinition {
  id: string;
  title: string;
  customTitle: boolean;
  layout: Layout;
  paneKeys: string[];
}

export interface WorkspaceDefinition {
  tabs: WorkspaceTabDefinition[];
  panes: WorkspacePaneDefinition[];
  activeTabId: string;
  activePaneKey: string | null;
}

export interface WorkspaceProfile extends WorkspaceDefinition {
  id: string;
  name: string;
}

export interface MultiplexerAvailability {
  kind: MultiplexerKind;
  available: boolean;
  version: string | null;
}
