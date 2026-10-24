import { useEffect, useState } from "react";
import { RpcError } from "../../rpc/client";
import type { RenderedNote } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
import { useRpcSession } from "../../rpc/RpcProvider";

import { RenderedPreview } from "./RenderedPreview";

const DEBOUNCE_MS = 250;
const decode = (value: string) => {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
};
/**
 * Rendered Markdown of the current buffer (`notes.render` with the note path, so relative
 * images resolve from its folder). Latest request wins and results are not cached: a render
 * can be several MiB. Links never navigate the app window.
 */
export function Preview({ path, text, onOpenNote }: { path: string; text: string; onOpenNote(path: string): void }) {
  const { client, state } = useRpcSession();
  const [result, setResult] = useState<RenderedNote>();
  const [error, setError] = useState<string>();
  useEffect(() => {
    if (state !== "ready") return;
    let live = true;
    const timer = setTimeout(() => {
      client.call("notes.render", { path, text }).then(
        (rendered) => {
          if (!live) return;
          setResult(rendered);
          setError(undefined);
        },
        (failure) => {
          if (!live) return;
          const e = failure instanceof RpcError ? failure : new RpcError("internal");
          setError(messageFor(e.code, e.detail).title);
        },
      );
    }, DEBOUNCE_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [client, state, path, text]);
  // Links carry `data-href` only (see `sanitizePreview`): web links open outside, a plain relative
  // `.md` link opens that note, and anything else (`/\host`, `//host`, other schemes) does nothing.
  const follow = (href: string) => {
    const target = decode(href.split("#")[0] ?? "");
    if (!target || /^[a-z][a-z0-9+.-]*:|^[/\\]/i.test(target) || !/\.md$/i.test(target)) return;
    void client.call("notes.resolve", { target, kind: "md", from: path }).then(
      ({ paths, ambiguous }) => {
        if (!ambiguous && paths[0]) onOpenNote(paths[0]);
      },
      () => {},
    );
  };
  return <RenderedPreview result={result} error={error} onLink={follow} />;
}
