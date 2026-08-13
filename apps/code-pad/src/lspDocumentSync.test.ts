import { describe, expect, it, vi } from "vitest";
import type { LanguageServerStatus, LspConfig, LspDidOpen } from "./types";
import {
  languageIdForPath,
  LspDocumentSync,
  type LspDocumentSnapshot,
  type LspDocumentTransport,
} from "./lspDocumentSync";

function config(overrides: Partial<LspConfig> = {}): LspConfig {
  return {
    version: 1,
    enabled: true,
    workspace_root: "/work",
    server_by_language: {
      rust: { kind: "local", installed_path: "/tools/rust-analyzer", args: [] },
    },
    custom_servers: [],
    update_policy: "manual",
    ...overrides,
  };
}

function document(text = "fn main() {}", overrides: Partial<LspDocumentSnapshot> = {}): LspDocumentSnapshot {
  return {
    id: "doc-1",
    path: "/work/src/main.rs",
    text,
    dirty: text !== "fn main() {}",
    ...overrides,
  };
}

function transportFor(
  calls: string[],
  loadedConfig: LspConfig = config(),
): LspDocumentTransport {
  return {
    loadConfig: vi.fn().mockResolvedValue({ config: loadedConfig, persist_allowed: true, error: null }),
    statuses: vi.fn().mockResolvedValue([] as LanguageServerStatus[]),
    start: vi.fn(async (languageId: string) => {
      calls.push(`start:${languageId}`);
    }),
    stop: vi.fn(async (languageId: string) => {
      calls.push(`stop:${languageId}`);
    }),
    open: vi.fn(async (languageId: string, path: string, text: string) => {
      calls.push(`open:${languageId}:${path}:${text}`);
      return { uri: `file://${path}`, languageId, version: 1, text };
    }),
    change: vi.fn(async (languageId: string, uri: string, text: string, dirty: boolean) => {
      calls.push(`change:${languageId}:${uri}:${text}:${dirty}`);
      return { uri, version: 2, contentChanges: [{ text }] };
    }),
    reload: vi.fn(async (languageId: string, uri: string, text: string) => {
      calls.push(`reload:${languageId}:${uri}:${text}`);
      return { uri, version: 2, contentChanges: [{ text }] };
    }),
    save: vi.fn(async (languageId: string, uri: string) => {
      calls.push(`save:${languageId}:${uri}`);
      return { uri, version: 2 };
    }),
    close: vi.fn(async (languageId: string, uri: string) => {
      calls.push(`close:${languageId}:${uri}`);
      return { uri };
    }),
  };
}

describe("LSP document language mapping", () => {
  it("maps only the exact supported extensions", () => {
    expect(languageIdForPath("C:\\work\\main.rs")).toBe("rust");
    expect(languageIdForPath("/work/app.ts")).toBe("typescript");
    expect(languageIdForPath("/work/app.tsx")).toBe("typescriptreact");
    expect(languageIdForPath("/work/app.js")).toBe("javascript");
    expect(languageIdForPath("/work/app.jsx")).toBe("javascriptreact");
    expect(languageIdForPath("/work/app.py")).toBe("python");
    expect(languageIdForPath("/work/stubs.pyi")).toBe("python");
    expect(languageIdForPath("/work/config.json")).toBe("json");
    expect(languageIdForPath("/work/config.JSONC")).toBe("jsonc");
    expect(languageIdForPath("/work/index.html")).toBe("html");
    expect(languageIdForPath("/work/index.htm")).toBe("html");
    expect(languageIdForPath("/work/site.css")).toBe("css");
    expect(languageIdForPath("/work/site.scss")).toBe("scss");
    expect(languageIdForPath("/work/site.less")).toBe("less");
    expect(languageIdForPath("/work/README.md")).toBeNull();
    expect(languageIdForPath("/work/.rs")).toBe("rust");
    expect(languageIdForPath("/work/no-extension")).toBeNull();
  });
});

describe("LspDocumentSync", () => {
  it("falls back without invoking native LSP when disabled, unconfigured, or outside the workspace", async () => {
    const calls: string[] = [];
    const transport = transportFor(calls, config({ enabled: false }));
    const sync = new LspDocumentSync(transport);
    await sync.setWorkspace("/work");
    await sync.setConfig(config({ enabled: false }));
    await sync.open(document());
    await sync.change({ ...document("edited"), dirty: true });
    await sync.flush();
    expect(calls).toEqual([]);

    await sync.setConfig(config({ enabled: true, server_by_language: {} }));
    expect(sync.getState().configuredLanguages).toEqual([]);
    await sync.setConfig(config({ enabled: true, workspace_root: "/other" }));
    expect(sync.getState().workspaceReady).toBe(false);
    expect(calls).toEqual([]);
  });

  it("starts lazily and preserves open/change/save/close order for every transaction", async () => {
    const calls: string[] = [];
    const sync = new LspDocumentSync(transportFor(calls));
    await sync.setWorkspace("/work");
    await sync.setConfig(config());

    const first = document();
    const second = document("fn main() { println!(\"one\"); }", { dirty: true });
    const third = document("fn main() { println!(\"two\"); }", { dirty: true });
    await sync.open(first);
    const changes = [sync.change(second), sync.change(third)];
    const save = sync.save(first.id);
    const close = sync.close(first.id);
    await Promise.all([...changes, save, close]);

    expect(calls).toEqual([
      "start:rust",
      "open:rust:/work/src/main.rs:fn main() {}",
      "change:rust:file:///work/src/main.rs:fn main() { println!(\"one\"); }:true",
      "change:rust:file:///work/src/main.rs:fn main() { println!(\"two\"); }:true",
      "save:rust:file:///work/src/main.rs",
      "close:rust:file:///work/src/main.rs",
    ]);
  });

  it("shares the initial config load across documents opened concurrently", async () => {
    const calls: string[] = [];
    const transport = transportFor(calls);
    const sync = new LspDocumentSync(transport);
    await sync.setWorkspace("/work");
    await Promise.all([
      sync.open(document()),
      sync.open(document("fn second() {}", { id: "doc-2", path: "/work/src/second.rs", dirty: false })),
    ]);

    expect(transport.loadConfig).toHaveBeenCalledTimes(1);
    expect(transport.open).toHaveBeenCalledTimes(2);
  });

  it("does not let a stale open response enter a new generation", async () => {
    const calls: string[] = [];
    const transport = transportFor(calls);
    let resolveOpen!: (value: { uri: string; languageId: string; version: number; text: string }) => void;
    transport.open = vi.fn((languageId: string, path: string, text: string): Promise<LspDidOpen> => {
      calls.push(`open:${languageId}:${path}:${text}`);
      return new Promise((resolve) => { resolveOpen = resolve; });
    });
    const sync = new LspDocumentSync(transport);
    await sync.setWorkspace("/work");
    await sync.setConfig(config());
    const opening = sync.open(document());
    await vi.waitFor(() => expect(transport.open).toHaveBeenCalledTimes(1));

    const switched = sync.setWorkspace("/other");
    resolveOpen({ uri: "file:///work/src/main.rs", languageId: "rust", version: 1, text: "fn main() {}" });
    await Promise.all([opening, switched]);
    await sync.flush();

    expect(calls).toContain("close:rust:file:///work/src/main.rs");
    expect(calls).toContain("stop:rust");
    expect(calls.filter((call) => call.startsWith("open:")).length).toBe(1);
  });

  it("reopens the latest document snapshot after a workspace/config transition", async () => {
    const calls: string[] = [];
    const transport = transportFor(calls);
    const sync = new LspDocumentSync(transport);
    await sync.setWorkspace("/work");
    await sync.setConfig(config());
    await sync.open(document());

    const nextConfig = config({ workspace_root: "/other" });
    await sync.setWorkspace("/other");
    await sync.setConfig(nextConfig);
    await sync.flush();

    expect(calls).toEqual([
      "start:rust",
      "open:rust:/work/src/main.rs:fn main() {}",
      "close:rust:file:///work/src/main.rs",
      "stop:rust",
    ]);
  });

  it("uses didChange for an external reload without marking the buffer dirty", async () => {
    const calls: string[] = [];
    const transport = transportFor(calls);
    const sync = new LspDocumentSync(transport);
    await sync.setWorkspace("/work");
    await sync.setConfig(config());
    await sync.open(document());
    await sync.reload({ ...document("from disk"), dirty: false });
    await sync.flush();

    expect(calls[calls.length - 1]).toBe("reload:rust:file:///work/src/main.rs:from disk");
  });

  it("records transport failures as state without rejecting editor operations", async () => {
    const calls: string[] = [];
    const transport = transportFor(calls);
    transport.open = vi.fn().mockRejectedValue(new Error("server exited"));
    const sync = new LspDocumentSync(transport);
    await sync.setWorkspace("/work");
    await sync.setConfig(config());
    await expect(sync.open(document())).resolves.toBeUndefined();
    expect(sync.getState().lastError).toBe("server exited");
  });
});
