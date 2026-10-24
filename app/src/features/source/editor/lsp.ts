import type { Extension } from "@codemirror/state";
import { useEffect, useState } from "react";
import type { DocKey, Language } from "../../../rpc/gen/rpc";
import { useRpcSession } from "../../../rpc/RpcProvider";
import { useDocuments } from "../../documents/store";
import { detectLanguage } from "./languages";
import { connectLsp, type EditorLspSession } from "./lspSession";
import { projectUris } from "./lspWorkspace";

export function lspLanguage(path: string, text: string): { server: Language; id: string } | undefined {
  const lang = detectLanguage(path, text);
  const aliases: Record<string, Language> = {
    rust: "rust",
    typescript: "typescript",
    javascript: "typescript",
    jsx: "typescript",
    tsx: "typescript",
    python: "python",
    go: "go",
    shell: "bash",
    bash: "bash",
    yaml: "yaml",
    toml: "toml",
    json: "json",
  };
  const server = lang ? aliases[lang] : undefined;
  if (!server) return;
  return {
    server,
    id: /\.tsx$/i.test(path)
      ? "typescriptreact"
      : /\.jsx$/i.test(path)
        ? "javascriptreact"
        : lang === "shell"
          ? "shellscript"
          : lang!,
  };
}

/** An immutable client per stream generation prevents old requests from mutating a new editor. */
export function useEditorLsp(key: DocKey) {
  const { client, state } = useRpcSession();
  const doc = useDocuments((s) =>
    Object.values(s.docs).find((d) => d.key.root === key.root && d.key.path === key.path),
  );
  const language = lspLanguage(key.path, doc?.text ?? "");
  const server = language?.server,
    languageId = language?.id;
  const lease = doc?.leaseId;
  const [extension, setExtension] = useState<Extension>([]);
  const [message, setMessage] = useState<string>();
  useEffect(() => {
    setExtension([]);
    setMessage(undefined);
    if (!client || state !== "ready" || !server || !languageId || !lease) return;
    let attempt = 0;
    let live = true,
      session: EditorLspSession | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const controller = new AbortController();
    const retry = (own: number) => {
      if (!live || timer || own !== attempt) return;
      setExtension([]);
      timer = setTimeout(() => {
        timer = undefined;
        void start();
      }, 1500);
    };
    const start = async () => {
      const own = ++attempt;
      try {
        const projects = await client.call("projects.list", {});
        if (!live) return;
        const projectId = key.root.slice("project:".length),
          project = projects.items.find((p) => p.id === projectId);
        if (!project) return;
        const next = await connectLsp(
          client,
          { projectId, language: server },
          projectUris(projectId, project.path),
          () => retry(own),
          (text) => {
            if (live) setMessage(text);
          },
          controller.signal,
        );
        if (!live || own !== attempt) {
          next.close();
          return;
        }
        session = next;
        setExtension(next.extension({ root: key.root, path: key.path }, languageId));
      } catch {
        retry(own);
      }
    };
    void start();
    return () => {
      live = false;
      clearTimeout(timer);
      controller.abort();
      session?.close();
    };
  }, [client, state, key.root, key.path, server, languageId, lease]);
  return { extension, message, language: server };
}
