import { describe, expect, it, vi } from "vitest";
import type {
  AppliedDocumentEdits,
  LanguageServerStatus,
  LspCompletionResult,
  LspConfig,
  LspDiagnosticResult,
  LspDidOpen,
  LspFeatureResponse,
  LspFilteredLocations,
  LspHoverResult,
} from "./types";
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

function readyStatus(overrides: Partial<LanguageServerStatus> = {}): LanguageServerStatus {
  return {
    languageId: "rust",
    status: "ready",
    processState: "running",
    serverInfo: null,
    capabilities: {
      positionEncoding: "utf-16",
      legacyPositionEncoding: false,
      syncKind: "full",
      openClose: true,
      save: true,
      completion: true,
      hover: true,
      definition: true,
      references: true,
      rename: true,
      formatting: true,
      diagnostics: true,
    },
    documentCount: 1,
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
    pullDiagnostics: vi.fn(async (_languageId: string, uri: string): Promise<LspFeatureResponse<LspDiagnosticResult>> => ({
      metadata: { uri, version: 1 },
      value: { uri, version: 1, diagnostics: [], origin: "pull" },
      stale: false,
    })),
    completion: vi.fn(async (_languageId: string, uri: string): Promise<LspFeatureResponse<LspCompletionResult>> => ({
      metadata: { uri, version: 1 },
      value: { isIncomplete: false, items: [] },
      stale: false,
    })),
    hover: vi.fn(async (_languageId: string, uri: string): Promise<LspFeatureResponse<LspHoverResult | null>> => ({
      metadata: { uri, version: 1 },
      value: null,
      stale: false,
    })),
    definition: vi.fn(async (_languageId: string, uri: string): Promise<LspFeatureResponse<LspFilteredLocations>> => ({
      metadata: { uri, version: 1 },
      value: { locations: [], rejected: 0 },
      stale: false,
    })),
    references: vi.fn(async (_languageId: string, uri: string): Promise<LspFeatureResponse<LspFilteredLocations>> => ({
      metadata: { uri, version: 1 },
      value: { locations: [], rejected: 0 },
      stale: false,
    })),
    rename: vi.fn(async (): Promise<AppliedDocumentEdits> => ({ documents: [] })),
    formatting: vi.fn(async (): Promise<AppliedDocumentEdits> => ({ documents: [] })),
    restart: vi.fn(async (languageId: string) => {
      calls.push(`restart:${languageId}`);
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

  it("debounces diagnostics after open, change, and save into one pull", async () => {
    vi.useFakeTimers();
    try {
      const calls: string[] = [];
      const transport = transportFor(calls);
      transport.statuses = vi.fn().mockResolvedValue([readyStatus()]);
      const sync = new LspDocumentSync(transport);
      await sync.setWorkspace("/work");
      await sync.setConfig(config());
      await sync.open(document());
      await sync.change(document("edited", { dirty: true }));
      await sync.save("doc-1");
      await vi.advanceTimersByTimeAsync(151);
      await sync.flush();
      expect(transport.pullDiagnostics).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("marks a late diagnostic event stale after the native mirror advances", async () => {
    const calls: string[] = [];
    const transport = transportFor(calls);
    transport.statuses = vi.fn().mockResolvedValue([readyStatus()]);
    const sync = new LspDocumentSync(transport);
    await sync.setWorkspace("/work");
    await sync.setConfig(config());
    await sync.open(document());
    const received: boolean[] = [];
    sync.subscribeDiagnostics((snapshot) => received.push(snapshot.response.stale));
    sync.acceptDiagnosticsEvent({
      languageId: "rust",
      response: {
        metadata: { uri: "file:///work/src/main.rs", version: 1 },
        value: {
          uri: "file:///work/src/main.rs",
          version: 1,
          diagnostics: [{
            range: { start: { line: 0, character: 0 }, end: { line: 0, character: 2 } },
            message: "old",
          }],
          origin: "push",
        },
        stale: false,
      },
    });
    await sync.change(document("edited", { dirty: true }));
    sync.acceptDiagnosticsEvent({
      languageId: "rust",
      response: {
        metadata: { uri: "file:///work/src/main.rs", version: 1 },
        value: {
          uri: "file:///work/src/main.rs",
          version: 1,
          diagnostics: [],
          origin: "push",
        },
        stale: false,
      },
    });
    expect(sync.getDiagnostics("doc-1")?.response.stale).toBe(true);
    expect(sync.getDiagnostics("doc-1")?.response.value.diagnostics[0]?.message).toBe("old");
    // The editor mutation itself marks the previous result stale before the
    // late push arrives, so the subscriber observes two stale snapshots.
    expect(received).toEqual([false, true, true]);
    sync.applyDocuments([{ uri: "file:///work/src/main.rs", version: 3, text: "new" }]);
    expect(sync.getDiagnostics("doc-1")).toBeNull();
    expect(sync.getState().staleDiagnostics).toBe(false);
  });

  it("ignores a lower-version push while the cached diagnostics are current", async () => {
    const calls: string[] = [];
    const transport = transportFor(calls);
    transport.statuses = vi.fn().mockResolvedValue([readyStatus()]);
    const sync = new LspDocumentSync(transport);
    await sync.setWorkspace("/work");
    await sync.setConfig(config());
    await sync.open(document());
    const received: boolean[] = [];
    sync.subscribeDiagnostics((snapshot) => received.push(snapshot.response.stale));
    sync.acceptDiagnosticsEvent({
      languageId: "rust",
      response: {
        metadata: { uri: "file:///work/src/main.rs", version: 1 },
        value: {
          uri: "file:///work/src/main.rs",
          version: 1,
          diagnostics: [{
            range: { start: { line: 0, character: 0 }, end: { line: 0, character: 2 } },
            message: "current",
          }],
          origin: "push",
        },
        stale: false,
      },
    });
    sync.acceptDiagnosticsEvent({
      languageId: "rust",
      response: {
        metadata: { uri: "file:///work/src/main.rs", version: 0 },
        value: {
          uri: "file:///work/src/main.rs",
          version: 0,
          diagnostics: [],
          origin: "push",
        },
        stale: false,
      },
    });

    expect(sync.getDiagnostics("doc-1")?.response.value.diagnostics[0]?.message).toBe("current");
    expect(sync.getDiagnostics("doc-1")?.response.stale).toBe(false);
    expect(sync.getState().staleDiagnostics).toBe(false);
    expect(received).toEqual([false]);
  });

  it("ignores versionless push diagnostics even for the initial document", async () => {
    const calls: string[] = [];
    const transport = transportFor(calls);
    const sync = new LspDocumentSync(transport);
    await sync.setWorkspace("/work");
    await sync.setConfig(config());
    await sync.open(document());

    const received: unknown[] = [];
    sync.subscribeDiagnostics((snapshot) => received.push(snapshot));
    sync.acceptDiagnosticsEvent({
      languageId: "rust",
      response: {
        metadata: { uri: "file:///work/src/main.rs", version: 1 },
        value: {
          uri: "file:///work/src/main.rs",
          version: null,
          diagnostics: [],
          origin: "push",
        },
        stale: false,
      },
    });

    expect(received).toEqual([]);
    expect(sync.getDiagnostics("doc-1")).toBeNull();
  });

  it("cancels completion results when the next input arrives", async () => {
    const calls: string[] = [];
    const transport = transportFor(calls);
    transport.statuses = vi.fn().mockResolvedValue([readyStatus()]);
    let resolveFirst!: (value: LspFeatureResponse<LspCompletionResult>) => void;
    let resolveSecond!: (value: LspFeatureResponse<LspCompletionResult>) => void;
    let completionCalls = 0;
    transport.completion = vi.fn(async (_languageId, _uri, _position): Promise<LspFeatureResponse<LspCompletionResult>> => {
      completionCalls += 1;
      return new Promise((resolve) => {
        if (completionCalls === 1) resolveFirst = resolve;
        else resolveSecond = resolve;
      });
    });
    const sync = new LspDocumentSync(transport);
    await sync.setWorkspace("/work");
    await sync.setConfig(config());
    await sync.open(document());
    const first = sync.requestCompletion("doc-1", 1);
    await vi.waitFor(() => expect(transport.completion).toHaveBeenCalledTimes(1));
    const second = sync.requestCompletion("doc-1", 2);
    resolveFirst({ metadata: { uri: "file:///work/src/main.rs", version: 1 }, value: { isIncomplete: false, items: [] }, stale: false });
    await expect(first).resolves.toBeNull();
    await vi.waitFor(() => expect(transport.completion).toHaveBeenCalledTimes(2));
    resolveSecond({ metadata: { uri: "file:///work/src/main.rs", version: 1 }, value: { isIncomplete: false, items: [] }, stale: false });
    await expect(second).resolves.toMatchObject({ value: { items: [] } });
  });
});
