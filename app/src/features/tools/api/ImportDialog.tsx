/**
 * 가져오기: 붙여넣기·파일·OpenAPI URL → 미리보기(만들 파일·경고) → 저장할 곳 → `expect: absent`로 쓰기.
 * 중간에 실패하면 이번에 만든 파일만 지울지 한 번 묻는다(`api.collection_delete`는 확인 정책 always).
 * 검토는 기록하지 않는다. 미리보기는 파일 내용을 보여 주지 않으므로, 비밀 참조·파일 경로가 있는 프로젝트 파일은
 * 사용자가 열어 확인([검토하고 보내기]·[검토하고 저장])하기 전까지 송신·실행에 쓰이지 않는다(07b 34행).
 */
import { useState } from "react";
import type { CollectionRoot } from "../../../rpc/gen/rpc";
import { messageFor } from "../../../rpc/messages";
import { Banner, Button, Dialog } from "../../../ui";
import s from "./Api.module.css";
import { SendFailure, useApiBackend } from "./backend";
import { fetchOpenApi, type ImportPlan, importErrorText, planImport } from "./importPlan";
import { rollbackImport, type WrittenFile, writeImport } from "./lib/importTransaction";
import type { OpenApiFormat } from "./lib/openapi/shared";
import { bumpCollections } from "./useCollections";

interface Failed {
  root: CollectionRoot;
  written: WrittenFile[];
  reason: string;
}

const failureText = (error: unknown) =>
  error instanceof SendFailure
    ? (messageFor(error.code as never, error.detail)?.title ?? "파일을 쓰지 못했습니다.")
    : "파일을 쓰지 못했습니다.";

export function ImportDialog({
  open,
  onOpenChange,
  roots,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  roots: { id: string; label: string; root: CollectionRoot }[];
}) {
  const backend = useApiBackend();
  const [text, setText] = useState("");
  const [fileName, setFileName] = useState("");
  const [openApi, setOpenApi] = useState<OpenApiFormat>();
  const [url, setUrl] = useState("");
  const [plan, setPlan] = useState<ImportPlan>();
  const [rootId, setRootId] = useState(roots[0]?.id ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [notice, setNotice] = useState<string>();
  const [failed, setFailed] = useState<Failed>();
  const root = roots.find((item) => item.id === rootId)?.root ?? roots[0]?.root;

  const reset = () => {
    setPlan(undefined);
    setError(undefined);
    setNotice(undefined);
  };
  const preview = () => {
    reset();
    try {
      setPlan(planImport(text, fileName, openApi));
    } catch (cause) {
      setError(importErrorText(cause));
    }
  };
  const fromUrl = async () => {
    reset();
    setBusy(true);
    try {
      const fetched = await fetchOpenApi(backend, url, new AbortController().signal);
      setText(fetched.text);
      setFileName("");
      setOpenApi(fetched.format);
    } catch (cause) {
      setError(cause instanceof SendFailure ? failureText(cause) : importErrorText(cause));
    } finally {
      setBusy(false);
    }
  };
  const run = async () => {
    if (!plan || !root) return;
    setBusy(true);
    setNotice(undefined);
    try {
      const files = plan.files.map((file) => ({ path: file.path, text: file.text(root) }));
      const result = await writeImport(backend, root, files);
      if (result.failed) {
        const reason = `${result.failed.path}: ${failureText(result.failed.error)}`;
        if (result.written.length) setFailed({ root, written: result.written, reason });
        else setError(reason);
        return;
      }
      const requests = plan.files.filter((file) => file.kind === "request").length;
      const environments = plan.files.length - requests;
      setNotice(
        environments
          ? `요청 ${requests}개와 환경 ${environments}개를 가져왔습니다.`
          : `요청 ${requests}개를 가져왔습니다.`,
      );
      setPlan(undefined);
    } catch (cause) {
      setError(failureText(cause));
    } finally {
      setBusy(false);
      bumpCollections();
    }
  };
  const resolveFailure = async (remove: boolean) => {
    const current = failed;
    setFailed(undefined);
    if (!current) return;
    if (!remove) {
      setNotice(`남긴 파일: ${current.written.map((file) => file.path).join(", ")}`);
    } else {
      const { removed, kept } = await rollbackImport(backend, current.root, current.written);
      setNotice(
        `만든 파일 ${removed.length}개를 지웠습니다.${kept.length ? ` 그 사이 바뀌어 남긴 파일: ${kept.join(", ")}` : ""}`,
      );
    }
    setError(current.reason);
    setPlan(undefined);
    bumpCollections();
  };

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="가져오기"
      description="cURL·Postman·Insomnia·HAR·Bruno·OpenAPI를 요청 파일로 만듭니다. 직접 입력한 비밀은 가려서 저장합니다."
      width={640}
      footer={
        <>
          <Button onClick={() => onOpenChange(false)}>닫기</Button>
          <Button variant="primary" disabled={!plan || busy || !root} onClick={() => void run()}>
            가져오기
          </Button>
        </>
      }
    >
      <div className={s.table}>
        <textarea
          className={`${s.input} ${s.mono}`}
          style={{ height: 140, padding: "var(--space-2)" }}
          aria-label="가져올 내용"
          value={text}
          onChange={(event) => {
            setText(event.target.value);
            setOpenApi(undefined);
            setPlan(undefined);
          }}
        />
        <div className={s.bar}>
          <input
            type="file"
            aria-label="파일에서 읽기"
            onChange={async (event) => {
              const file = event.target.files?.[0];
              if (!file) return;
              setText(await file.text());
              setFileName(file.name);
              setOpenApi(undefined);
              setPlan(undefined);
            }}
          />
          <input
            className={`${s.input} ${s.grow}`}
            aria-label="OpenAPI URL"
            placeholder="https://…/openapi.json"
            value={url}
            onChange={(event) => setUrl(event.target.value)}
          />
          <Button size="sm" disabled={!url || busy} onClick={() => void fromUrl()}>
            URL에서 받기
          </Button>
          <Button size="sm" disabled={!text.trim() || busy} onClick={preview}>
            미리보기
          </Button>
        </div>
        {error && <Banner tone="danger">{error}</Banner>}
        {notice && <p role="status">{notice}</p>}
        {plan && (
          <>
            {plan.warnings.map((warning) => (
              <p key={warning} className={s.muted}>
                {warning}
              </p>
            ))}
            {plan.files.some((file) => file.review) && (
              <p className={s.edited}>
                {`가린 값이 있는 요청 ${plan.files.filter((file) => file.review).length}개는 검토 뒤 실행할 수 있습니다.`}
              </p>
            )}
            <ul aria-label="만들 파일" className={`${s.tree} ${s.mono}`}>
              {plan.files.map((file) => (
                <li key={file.path}>{file.path}</li>
              ))}
            </ul>
            <label className={s.bar}>
              저장할 곳
              <select className={s.select} value={rootId} onChange={(event) => setRootId(event.target.value)}>
                {roots.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.label}
                  </option>
                ))}
              </select>
            </label>
          </>
        )}
      </div>
      <Dialog
        open={!!failed}
        onOpenChange={(next) => !next && void resolveFailure(false)}
        title="가져오기를 끝내지 못했습니다"
        description={`이번에 만든 파일 ${failed?.written.length ?? 0}개를 지울까요? 그 사이 바뀐 파일은 남깁니다.`}
        footer={
          <>
            <Button onClick={() => void resolveFailure(false)}>파일 남기기</Button>
            <Button variant="danger" onClick={() => void resolveFailure(true)}>
              만든 파일 지우기
            </Button>
          </>
        }
      >
        <p className={s.muted}>{failed?.reason}</p>
      </Dialog>
    </Dialog>
  );
}
