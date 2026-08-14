import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/isTauri";
import type { RenderedDoc, SearchResult, TreeEntry } from "./types";

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

export async function writeFile(rel: string, content: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("write_file", { rel, content });
}

export async function createFile(rel: string, content?: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("create_file", { rel, content });
}

export async function renameFile(from: string, to: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("rename_file", { from, to });
}

export async function deleteFile(rel: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("delete_file", { rel });
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
