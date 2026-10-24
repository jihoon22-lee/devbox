/** 도구 화면 검색값: `{ view?, root?, path?, tool? }`(07b S5-3). `root`는 `personal` 또는 프로젝트 id. */
export type ToolsView = "api" | "webhooks" | "transforms" | "env";
export type ToolsSearch = {
  view?: ToolsView;
  root?: string;
  path?: string;
  tool?: "import" | "runner" | "websocket" | "sse" | "grpc";
};

const VIEWS = new Set<string>(["api", "webhooks", "transforms", "env"]);
const TOOLS = new Set<string>(["import", "runner", "websocket", "sse", "grpc"]);

export function toolsSearch(search: Record<string, unknown>): ToolsSearch {
  const out: ToolsSearch = {};
  if (typeof search.view === "string" && VIEWS.has(search.view)) out.view = search.view as ToolsView;
  if (typeof search.root === "string" && search.root.trim()) out.root = search.root;
  if (typeof search.path === "string" && search.path.trim()) out.path = search.path;
  if (typeof search.tool === "string" && TOOLS.has(search.tool)) out.tool = search.tool as ToolsSearch["tool"];
  return out;
}
