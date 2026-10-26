/** 도구 본문(API): 열린 요청 탭 + 요청 | 응답 분할. 1100px 미만이면 응답이 아래로 간다. */
import { useState } from "react";
import { Button, EmptyState, SplitPane, Tabs } from "../../../ui";
import s from "./Api.module.css";
import { CodegenDialog } from "./CodegenDialog";
import { HistoryList } from "./HistoryList";
import { RequestEditor } from "./RequestEditor";
import { RequestTabs } from "./RequestTabs";
import { ResponseNoteAction } from "./ResponseNoteAction";
import { ResponseView } from "./ResponseView";
import { useActiveTab, useApiTabs } from "./store";
import { useNarrow } from "./useNarrow";

export function ApiWorkspace() {
  const tab = useActiveTab();
  const environment = useApiTabs((state) => state.environment);
  const narrow = useNarrow(1100);
  const [ratio, setRatio] = useState(0.5);
  const [view, setView] = useState<"response" | "history">("response");
  const [codegen, setCodegen] = useState(false);
  const current = tab?.result ? new TextDecoder().decode(tab.result.response.body) : undefined;
  return (
    <div className={s.workspace}>
      <RequestTabs />
      {tab ? (
        <SplitPane dir={narrow ? "column" : "row"} ratio={ratio} onRatio={setRatio} label="요청·응답 크기 조절">
          <RequestEditor key={tab.id} tab={tab} />
          <div className={s.pane}>
            <Tabs
              label="응답 패널"
              items={[
                { id: "response", label: "응답" },
                { id: "history", label: "기록" },
              ]}
              active={view}
              onSelect={(id) => setView(id as "response" | "history")}
              actions={
                <Button size="sm" variant="ghost" onClick={() => setCodegen(true)}>
                  코드 생성
                </Button>
              }
            />
            {view === "response" ? (
              <ResponseView
                result={tab.result}
                draft={tab.draft}
                failure={tab.failure}
                noteAction={
                  tab.result && (
                    <ResponseNoteAction
                      historyId={tab.result.response.historyId}
                      ownerKey={`${tab.id}:${JSON.stringify(tab.root)}:${tab.result.response.sentAtMs}`}
                    />
                  )
                }
              />
            ) : (
              <HistoryList
                origin={tab.path ? { root: tab.root, path: tab.path } : null}
                current={current}
                revision={tab.result?.response.sentAtMs ?? 0}
              />
            )}
          </div>
        </SplitPane>
      ) : (
        <EmptyState title="열린 요청이 없습니다" body="목록에서 요청을 고르거나 새 요청을 만드세요." />
      )}
      {tab && (
        <CodegenDialog
          open={codegen}
          onOpenChange={setCodegen}
          draft={tab.draft}
          environment={environment?.variables ?? []}
        />
      )}
    </div>
  );
}
