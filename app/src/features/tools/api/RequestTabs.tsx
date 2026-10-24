/** 열린 요청 탭. 저장하지 않은 초안은 점으로 표시하고, 닫을 때 버릴지 묻는다. */
import { useState } from "react";
import { Button, ConfirmDialog, Tabs } from "../../../ui";
import s from "./Api.module.css";
import { isDirty, useApiTabs } from "./store";

export function DirtyMark() {
  return (
    <span className={s.dirty} role="img" aria-label="저장하지 않은 변경">
      ●
    </span>
  );
}

export function RequestTabs() {
  const tabs = useApiTabs((state) => state.tabs);
  const active = useApiTabs((state) => state.active);
  const { select, close, openNew } = useApiTabs.getState();
  const [closing, setClosing] = useState<string>();
  const closingTab = tabs.find((tab) => tab.key === closing);
  return (
    <>
      <Tabs
        label="열린 요청"
        items={tabs.map((tab) => ({
          id: tab.key,
          label: tab.name,
          badge: isDirty(tab) ? <DirtyMark /> : undefined,
          closable: true,
        }))}
        active={active}
        onSelect={select}
        onClose={(key) => {
          const tab = tabs.find((t) => t.key === key);
          if (tab && isDirty(tab)) setClosing(key);
          else close(key);
        }}
        actions={
          <Button
            size="sm"
            variant="ghost"
            onClick={() => openNew(tabs.find((t) => t.key === active)?.root ?? { kind: "personal" })}
          >
            새 요청
          </Button>
        }
      />
      <ConfirmDialog
        open={!!closingTab}
        title={`${closingTab?.name ?? ""} 탭을 닫을까요?`}
        body="저장하지 않은 변경은 사라집니다."
        confirmLabel="변경 버리고 닫기"
        onCancel={() => setClosing(undefined)}
        onConfirm={() => {
          if (closing) close(closing);
          setClosing(undefined);
        }}
      />
    </>
  );
}
