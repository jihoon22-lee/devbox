/** 코드 생성: curl·fetch·axios·python requests·Go·Rust reqwest. 비밀·민감 값은 `{{이름}}` 자리 표시로 남는다. */
import { useMemo, useState } from "react";
import type { Pair } from "../../../rpc/gen/rpc";
import { Button, Dialog } from "../../../ui";
import { CodeView } from "../../../ui/code/CodeView";
import s from "./Api.module.css";
import { CODE_TARGETS, type CodeTarget, generateCode } from "./lib/codegen";
import type { RequestDraft } from "./model/draft";
import { templateFromDraft } from "./model/toTemplate";

const LABELS: Record<CodeTarget, string> = {
  curl: "cURL",
  fetch: "JavaScript fetch",
  axios: "JavaScript axios",
  python: "Python requests",
  go: "Go net/http",
  rust: "Rust reqwest",
};

export function CodegenDialog({
  open,
  onOpenChange,
  draft,
  environment,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  draft: RequestDraft;
  environment: Pair[];
}) {
  const [target, setTarget] = useState<CodeTarget>("curl");
  const [copied, setCopied] = useState(false);
  const result = useMemo(() => {
    if (!open) return null;
    try {
      const variables = environment.map((pair) => ({ key: pair.name, value: pair.value, secret: false }));
      return generateCode(target, templateFromDraft(draft), variables);
    } catch {
      return null;
    }
  }, [open, target, draft, environment]);
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="코드 생성"
      width={720}
      footer={
        <>
          {copied && <span className={s.muted}>복사했습니다</span>}
          <Button
            disabled={!result}
            onClick={() =>
              result &&
              void navigator.clipboard.writeText(result.code).then(
                () => setCopied(true),
                () => setCopied(false),
              )
            }
          >
            복사
          </Button>
          <Button variant="primary" onClick={() => onOpenChange(false)}>
            닫기
          </Button>
        </>
      }
    >
      <label className={s.bar}>
        언어
        <select
          className={s.select}
          value={target}
          onChange={(event) => {
            setTarget(event.target.value as CodeTarget);
            setCopied(false);
          }}
        >
          {CODE_TARGETS.map((item) => (
            <option key={item} value={item}>
              {LABELS[item]}
            </option>
          ))}
        </select>
      </label>
      {result ? (
        <CodeView ariaLabel="생성한 코드" value={result.code} readOnly />
      ) : (
        <p role="alert">이 요청으로는 코드를 만들 수 없습니다. URL과 본문을 확인해 주세요.</p>
      )}
    </Dialog>
  );
}
