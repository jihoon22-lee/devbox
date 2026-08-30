/** The security-sensitive options applied to every Mermaid runtime instance. */
export interface MermaidInitializeConfig {
  startOnLoad: false;
  theme: "dark";
  securityLevel: "strict";
}

export interface MermaidRenderResult {
  svg: string;
}

/** The small part of Mermaid's API used by the preview components. */
export interface MermaidRenderer {
  initialize(config: MermaidInitializeConfig): void;
  render(id: string, source: string): Promise<MermaidRenderResult>;
}

const MERMAID_CONFIG: MermaidInitializeConfig = {
  startOnLoad: false,
  theme: "dark",
  securityLevel: "strict",
};

let rendererPromise: Promise<MermaidRenderer> | null = null;

async function importAndInitialize(): Promise<MermaidRenderer> {
  // Keep this import inside the first getter call. Importing this package must
  // not pull Mermaid into an app's initial editor chunk.
  const module = await import("mermaid");
  const renderer = module.default as unknown as MermaidRenderer;
  renderer.initialize(MERMAID_CONFIG);
  return renderer;
}

/**
 * Loads and initializes Mermaid on demand.
 *
 * The promise is retained after a successful load so all previews share one
 * initialized runtime. A failed import or initialization clears the promise,
 * allowing a later preview to retry.
 */
export function getMermaidRenderer(): Promise<MermaidRenderer> {
  if (!rendererPromise) {
    const pending = importAndInitialize();
    rendererPromise = pending.catch((error: unknown) => {
      rendererPromise = null;
      throw error;
    });
  }
  return rendererPromise;
}

/** Renders a diagram through the shared, lazily initialized runtime. */
export async function renderMermaid(id: string, source: string): Promise<MermaidRenderResult> {
  const renderer = await getMermaidRenderer();
  return renderer.render(id, source);
}
