/** 요청 편집: URL 줄, 쿼리·헤더·쿠키·본문·인증 탭, 저장·새 요청으로 저장(Ctrl+S), 보내기(Ctrl+Enter). */
import { useState } from "react";
import { messageFor } from "../../../rpc/messages";
import { Banner, Button, ConfirmDialog, Tabs } from "../../../ui";
import s from "./Api.module.css";
import { AuthEditor } from "./AuthEditor";
import { BodyEditor } from "./BodyEditor";
import type { RequestDraft, Row } from "./model/draft";
import { ParamTable } from "./ParamTable";
import { MaskedDialog, SaveAsNewDialog } from "./SaveDialogs";
import { type ApiTab, isDirty, useApiTabs } from "./store";
import { UrlBar } from "./UrlBar";
import { useSave } from "./useSave";
import { useSend } from "./useSend";

const SECTIONS = [
  { id: "query", label: "쿼리" },
  { id: "headers", label: "헤더" },
  { id: "cookies", label: "쿠키" },
  { id: "body", label: "본문" },
  { id: "auth", label: "인증" },
] as const;
type Section = (typeof SECTIONS)[number]["id"];
const ROWS = { query: "쿼리", headers: "헤더", cookies: "쿠키" } as const;
const ROW_ERRORS: Record<string, keyof typeof ROWS> = {
  "api.invalid_header": "headers",
  "api.invalid_cookie": "cookies",
};

function rowErrors(tab: ApiTab, list: keyof typeof ROWS): Record<number, string> {
  const failure = tab.failure;
  if (!failure || ROW_ERRORS[failure.code] !== list) return {};
  const index = (failure.detail as { index?: unknown } | undefined)?.index;
  return typeof index === "number" ? { [index]: messageFor(failure.code as never, failure.detail).title } : {};
}

export function RequestEditor({ tab }: { tab: ApiTab }) {
  const [section, setSection] = useState<Section>(() => (tab.draft.bodyKind === "graphql" ? "body" : "query"));
  const [confirmReload, setConfirmReload] = useState(false);
  const setDraft = (draft: RequestDraft) => useApiTabs.getState().setDraftById(tab.id, draft);
  const save = useSave(tab);
  const sender = useSend();
  const send = () => void sender.send(tab.id);
  const rows = (list: keyof typeof ROWS) => (
    <ParamTable
      label={ROWS[list]}
      rows={tab.draft[list]}
      errors={rowErrors(tab, list)}
      onChange={(next: Row[]) => setDraft({ ...tab.draft, [list]: next })}
    />
  );
  return (
    <div
      className={s.pane}
      onKeyDown={(event) => {
        if (!event.ctrlKey || event.altKey) return;
        if (event.key === "s" || event.key === "S") {
          event.preventDefault();
          if (event.shiftKey) save.saveAsNew();
          else save.save();
        } else if (event.key === "Enter") {
          event.preventDefault();
          send();
        }
      }}
    >
      <UrlBar
        method={tab.draft.method}
        url={tab.draft.url}
        sending={tab.sending === true}
        onMethod={(method) => setDraft({ ...tab.draft, method })}
        onUrl={(url) => setDraft({ ...tab.draft, url })}
        onSend={send}
        onCancel={() => sender.cancel(tab.id)}
      />
      <div className={s.bar}>
        <Button size="sm" disabled={save.busy} onClick={save.save}>
          저장
        </Button>
        <Button size="sm" disabled={save.busy} onClick={save.saveAsNew}>
          새 요청으로 저장
        </Button>
      </div>
      {!tab.reviewed && (
        <Banner tone="info">
          이 PC에서 아직 검토하지 않은 파일입니다. 비밀 참조·파일 경로를 확인한 뒤 저장하거나 보내면 검토로 기록합니다.
        </Banner>
      )}
      {tab.notice && (
        <Banner
          tone="warn"
          actions={
            tab.conflict ? (
              <Button size="sm" onClick={() => (isDirty(tab) ? setConfirmReload(true) : void save.reload())}>
                다시 읽기
              </Button>
            ) : undefined
          }
        >
          {tab.conflict ? `${tab.notice} 다시 읽으면 고친 내용은 사라집니다.` : tab.notice}
        </Banner>
      )}
      <Tabs
        label="요청 설정"
        items={SECTIONS.map((item) => ({ id: item.id, label: item.label }))}
        active={section}
        onSelect={(id) => setSection(id as Section)}
      />
      {section === "query" && rows("query")}
      {section === "headers" && rows("headers")}
      {section === "cookies" && rows("cookies")}
      {section === "body" && <BodyEditor draft={tab.draft} onChange={setDraft} />}
      {section === "auth" && <AuthEditor draft={tab.draft} onChange={setDraft} />}
      <SaveAsNewDialog
        open={save.askName}
        initial={tab.path ? `${tab.name} 복사` : tab.name}
        onCancel={save.cancelName}
        onSave={(name) => void save.confirmName(name)}
      />
      <ConfirmDialog
        open={save.pendingReview}
        title="검토하고 저장할까요?"
        body="이 PC에서 검토하지 않은 프로젝트 파일입니다. 저장하면 이 내용을 검토한 것으로 기록하고 실행기가 이 요청을 보냅니다."
        confirmLabel="검토하고 저장"
        tone="primary"
        onCancel={save.cancelReview}
        onConfirm={save.confirmReview}
      />
      <ConfirmDialog
        open={tab.askSendReview === true}
        title="검토하고 보낼까요?"
        body="이 PC에서 검토하지 않았거나 비밀 재검토 표시가 있는 파일입니다. 비밀 참조·파일 경로·TLS 경로를 확인했으면 검토로 기록하고 보냅니다."
        confirmLabel="검토하고 보내기"
        tone="primary"
        onCancel={() => sender.dismissReview(tab.id)}
        onConfirm={() => void sender.confirmReview(tab.id)}
      />
      <ConfirmDialog
        open={confirmReload}
        title="파일을 다시 읽을까요?"
        body="저장하지 않은 변경은 사라지고 파일의 지금 내용으로 바뀝니다."
        confirmLabel="다시 읽기"
        onCancel={() => setConfirmReload(false)}
        onConfirm={() => {
          setConfirmReload(false);
          void save.reload();
        }}
      />
      <MaskedDialog
        count={save.pending ? save.pending.masked.length : null}
        onCancel={save.cancelMask}
        onMask={save.maskAndSave}
        onMove={() => void save.moveAndSave()}
      />
    </div>
  );
}
