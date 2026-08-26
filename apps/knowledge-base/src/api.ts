import { invoke } from "@tauri-apps/api/core";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { isTauri } from "./lib/isTauri";
import type {
  Backlink,
  RenderedDoc,
  SearchResult,
  TreeEntry,
  WikilinkCandidate,
  WikilinkOccurrence,
} from "./types";

export type OpenTarget =
  | { kind: "path"; path: string; line: number | null; column: number | null }
  | { kind: "profile"; id: string }
  | { kind: "workspace"; path: string }
  | { kind: "query"; text: string };

export interface OpenRequest {
  target: OpenTarget;
  from: string | null;
}

export interface InboundNote {
  path: string;
  content: string;
}

export interface KnowledgeOpenTarget {
  id: string;
  displayName: string;
}

const MOCK_OPEN_TARGETS: KnowledgeOpenTarget[] = [
  { id: "code-pad", displayName: "Code Pad" },
  { id: "workbench", displayName: "Workbench" },
];

const MOCK_TREE: TreeEntry[] = [
  { path: "Projects", is_dir: true },
  { path: "Projects/FamilyCard.md", is_dir: false },
  { path: "Notes", is_dir: true },
  { path: "Notes/tauri-study.md", is_dir: false },
  { path: "Journal", is_dir: true },
  { path: "Journal/2026-08-11.md", is_dir: false },
];

export async function getRoot(): Promise<string> {
  if (!isTauri()) return "C:\\Users\\me\\Documents\\Knowledge";
  return invoke<string>("get_root");
}

export async function listTree(): Promise<TreeEntry[]> {
  if (!isTauri()) {
    return [
      { path: "Notes/2026-08-14.md", is_dir: false },
      { path: "Projects/devbox.md", is_dir: false },
    ];
  }
  return invoke<TreeEntry[]>("list_tree");
}

export async function onDocsChanged(cb: () => void): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  return listen("docs-changed", () => cb());
}

export async function readFile(rel: string): Promise<string> {
  if (!isTauri()) {
    return rel.endsWith(".md") ? "# Mock note\n\nEdit me." : "";
  }
  return invoke<string>("read_file", { rel });
}

export async function openInboundNote(path: string): Promise<InboundNote> {
  if (!isTauri()) {
    const normalized = path.replace(/\\/g, "/");
    return { path: normalized.split("/Knowledge/").pop() ?? normalized, content: "# Mock note\n" };
  }
  return invoke<InboundNote>("open_inbound_note", { path });
}

/** Cold start 또는 같은 실행 중 instance가 남긴 요청을 한 번 가져온다. */
export async function takePendingOpen(): Promise<OpenRequest | null> {
  if (!isTauri()) return null;
  return invoke<OpenRequest | null>("take_pending_open");
}

/** Hot-instance relaunch 알림. payload 대신 pending pull을 authoritative하게 사용한다. */
export async function onOpenRequest(cb: (request: OpenRequest) => void): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  return listen<OpenRequest>("devbox://open", (event) => cb(event.payload));
}

export async function writeFile(rel: string, content: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("write_file", { rel, content });
}

export async function createFile(rel: string, content?: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("create_file", { rel, content });
}

export async function createDirectory(rel: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("create_directory", { rel });
}

export async function renameFile(from: string, to: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("rename_file", { from, to });
}

export async function deleteFile(rel: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("delete_file", { rel });
}

export async function entryPath(rel: string): Promise<string> {
  if (!isTauri()) return rel;
  return invoke<string>("entry_path", { rel });
}

export async function revealEntry(rel: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("reveal_entry", { rel });
}

export async function openTargets(): Promise<KnowledgeOpenTarget[]> {
  if (!isTauri()) return MOCK_OPEN_TARGETS;
  return invoke<KnowledgeOpenTarget[]>("open_targets");
}

export async function openIn(appId: string, rel: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("open_in", { appId, rel });
}

/** 편집기 메뉴에서 사용자가 Paste를 선택한 순간에만 plain text를 읽는다. */
export async function readClipboardText(): Promise<string> {
  if (!isTauri()) return navigator.clipboard.readText();
  return readText();
}

export async function searchDocs(query: string): Promise<SearchResult[]> {
  if (!isTauri()) {
    return MOCK_TREE.filter((t) => !t.is_dir && t.path.toLowerCase().includes(query.toLowerCase())).map((t) => ({ path: t.path, title: t.path.split("/").pop() ?? t.path }));
  }
  return invoke<SearchResult[]>("search_docs", { query });
}

export async function listTags(): Promise<string[]> {
  if (!isTauri()) return ["rust", "tauri", "daily"];
  return invoke<string[]>("list_tags");
}

export async function analyzeWikilinks(content: string): Promise<WikilinkOccurrence[]> {
  if (!isTauri()) return [];
  return invoke<WikilinkOccurrence[]>("analyze_wikilinks", { content });
}

export async function wikilinkCandidates(query: string): Promise<WikilinkCandidate[]> {
  if (!isTauri()) {
    const normalized = query.trim().toLowerCase();
    return MOCK_TREE
      .filter((entry) => !entry.is_dir && entry.path.toLowerCase().endsWith(".md"))
      .filter((entry) => entry.path.toLowerCase().includes(normalized))
      .slice(0, 100)
      .map((entry) => ({
        path: entry.path,
        title: entry.path.split("/").pop()?.replace(/\.md$/iu, "") ?? entry.path,
        link_target: entry.path.replace(/\.md$/iu, ""),
      }));
  }
  return invoke<WikilinkCandidate[]>("wikilink_candidates", { query });
}

export async function backlinks(rel: string): Promise<Backlink[]> {
  if (!isTauri()) return [];
  return invoke<Backlink[]>("backlinks", { rel });
}

export async function dailyNote(): Promise<[string, string]> {
  if (!isTauri()) return ["Journal/2026-08-11.md", "# Today\n"];
  return invoke<[string, string]>("daily_note");
}

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

export async function renderMarkdown(rel: string, content: string): Promise<RenderedDoc> {
  if (!isTauri()) {
    // 목업: 실제 마크다운 파서 없이도 프리뷰 레이아웃을 확인할 수 있게 원문을 그대로 보여준다.
    return { title: null, tags: [], html: `<pre>${escapeHtml(content)}</pre>`, mermaid: [] };
  }
  return invoke<RenderedDoc>("render_markdown", { rel, content });
}

/** 외부 URL을 기본 브라우저로 연다. Tauri 밖(브라우저 미리보기)에서는 새 탭으로 대신 연다. */
export async function openExternal(url: string): Promise<void> {
  if (!isTauri()) {
    window.open(url, "_blank", "noopener,noreferrer");
    return;
  }
  const { openUrl } = await import("@tauri-apps/plugin-opener");
  await openUrl(url);
}
