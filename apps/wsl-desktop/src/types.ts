export type Layout = "grid" | "cols" | "rows";

/** 세션 하나. 소속 탭 정보는 갖지 않는다 — Tab.paneIds가 소속의 단일 진실 소스다. */
export interface Pane {
  id: string;
  distro: string;
}

/** 탭 하나. 항상 팬을 최소 1개 갖는다 (탭 생성은 첫 세션 시작 성공과 함께 일어나고,
 * 마지막 팬이 닫히면 탭도 함께 닫힌다). */
export interface Tab {
  id: string;
  title: string;
  layout: Layout;
  paneIds: string[];
}

export interface DistroInfo {
  name: string;
  version: number;
  default: boolean;
}

export interface ContainerInfo {
  id: string;
  name: string;
  image: string;
  status: string;
  ports: string;
}

export interface GitStatus {
  path: string;
  branch: string;
  changes: number;
  clean: boolean;
}
