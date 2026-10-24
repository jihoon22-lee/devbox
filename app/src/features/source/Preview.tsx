import { useEffect, useState } from "react";
import type { DocKey, SourcePreview } from "../../rpc/gen/rpc";
import { useRpcSession } from "../../rpc/RpcProvider";
import { RenderedPreview } from "../notes/RenderedPreview";
import { previewLink, svgBlob } from "./preview";
import s from "./source.module.css";
import { useSource } from "./store";

export function Preview({ docKey, text }: { docKey: DocKey; text: string }) {
  const { client, state } = useRpcSession();
  const [result, setResult] = useState<{ root: string; path: string; text: string; rendered: SourcePreview }>();
  const [error, setError] = useState<string>();
  useEffect(() => {
    setError(undefined);
    if (state !== "ready") return;
    let live = true;
    const key = { root: docKey.root, path: docKey.path };
    const timer = setTimeout(() => {
      void client.call("files.preview", { key, text }).then(
        (rendered) => {
          if (live) setResult({ ...key, text, rendered });
        },
        () => {
          if (live) setError("미리보기를 불러오지 못했습니다.");
        },
      );
    }, 250);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [client, state, docKey.root, docKey.path, text]);
  const current =
    result?.root === docKey.root && result.path === docKey.path && result.text === text ? result.rendered : undefined;
  const follow = (href: string) => {
    const target = previewLink(docKey, href);
    if (target)
      void useSource
        .getState()
        .open(target, { pinned: true })
        .catch(() => setError("파일을 열지 못했습니다."));
  };
  if (current?.kind === "svg") return <SvgPreview text={current.text} />;
  return (
    <RenderedPreview
      result={current?.kind === "markdown" ? current.rendered : undefined}
      error={error}
      onLink={follow}
    />
  );
}
function SvgPreview({ text }: { text: string }) {
  const [url, setUrl] = useState<string>();
  const [error, setError] = useState(false);
  useEffect(() => {
    let owned: string | undefined;
    try {
      owned = URL.createObjectURL(svgBlob(text));
      setUrl(owned);
      setError(false);
    } catch {
      setError(true);
      setUrl(undefined);
    }
    return () => {
      if (owned) URL.revokeObjectURL(owned);
    };
  }, [text]);
  return (
    <section aria-label="미리보기" className={s.svgPreview}>
      {error ? <p role="alert">SVG를 표시할 수 없습니다.</p> : url && <img src={url} alt="SVG 미리보기" />}
    </section>
  );
}
