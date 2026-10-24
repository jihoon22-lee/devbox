import { type DragEvent, useState } from "react";
import type { DocKey } from "../../../rpc/gen/rpc";
import { Button, EmptyState, useToasts } from "../../../ui";
import { chooseDocumentClose } from "../../documents/closePrompt";
import { keyId } from "../../documents/offlineJournal";
import { needsCloseChoice, useDocuments } from "../../documents/store";
import { openPopout } from "../../terminal/popout";
import s from "../source.module.css";
import { useSource } from "../store";
import { CodeEditor } from "./CodeEditor";
import type { Group } from "./tabs";

export async function closeCodeTab(key: DocKey) {
  const doc = useDocuments.getState().find(key);
  const choices =
    doc && needsCloseChoice(doc) ? await chooseDocumentClose([doc], new AbortController().signal) : undefined;
  if (doc && needsCloseChoice(doc) && !choices) return;
  if (!(await useSource.getState().close(key, doc ? choices?.[doc.id] : undefined)))
    useToasts.getState().push({ tone: "warn", title: "파일을 닫지 못했습니다. 저장 상태를 확인하세요." });
}
export function CodeTabs({ root }: { root: string }) {
  const layout = useSource((s) => s.layout);
  const [dragging, setDragging] = useState(false);
  const groups: Group[] = layout.tabs.some((t) => t.group === 1 && t.key.root === root) ? [0, 1] : [0];
  return (
    <div className={s.groups} onDragStartCapture={() => setDragging(true)} onDragEndCapture={() => setDragging(false)}>
      {groups.map((group) => (
        <EditorGroup key={group} group={group} root={root} />
      ))}
      {dragging && groups.length === 1 && (
        <section
          className={s.splitDrop}
          aria-label="오른쪽으로 분할"
          onDragOver={(event) => event.preventDefault()}
          onDrop={(event) => {
            event.preventDefault();
            const id = event.dataTransfer.getData("application/x-devbox-code-tab");
            const tab = layout.tabs.find((tab) => tab.id === id && tab.key.root === root);
            if (tab) useSource.getState().send({ type: "move", key: tab.key, group: 1, index: 0 });
            setDragging(false);
          }}
        >
          이쪽으로 끌어 편집기를 나눕니다
        </section>
      )}
    </div>
  );
}
function EditorGroup({ group, root }: { group: Group; root: string }) {
  const { layout, locations, send } = useSource();
  const [dragging, setDragging] = useState<string>();
  const tabs = layout.tabs.filter((t) => t.group === group && t.key.root === root);
  const active = tabs.find((t) => t.id === layout.active[group]) ?? tabs[0];
  const move = (id: string, index: number) => {
    const tab = layout.tabs.find((t) => t.id === id);
    if (tab) send({ type: "move", key: tab.key, group, index });
  };
  const drop = (event: DragEvent, index: number) => {
    event.preventDefault();
    const id = event.dataTransfer.getData("application/x-devbox-code-tab") || dragging;
    if (id) move(id, index);
    setDragging(undefined);
  };
  return (
    <section
      className={s.group}
      aria-label={`${group === 0 ? "왼쪽" : "오른쪽"} 편집기`}
      onDragOver={(e) => e.preventDefault()}
      onDrop={(e) => drop(e, tabs.length)}
    >
      <div className={s.tabs} role="tablist" aria-label="파일 탭">
        {tabs.map((tab, index) => (
          <div
            key={tab.id}
            className={s.tab}
            data-active={tab.id === active?.id}
            data-preview={!tab.pinned}
            draggable
            onDragStart={(e) => {
              setDragging(tab.id);
              e.dataTransfer.setData("application/x-devbox-code-tab", tab.id);
            }}
            onDragEnd={() => setDragging(undefined)}
            onDragOver={(e) => e.preventDefault()}
            onDrop={(e) => {
              e.stopPropagation();
              drop(e, index);
            }}
          >
            <button
              type="button"
              role="tab"
              aria-selected={tab.id === active?.id}
              title={`${tab.key.root}/${tab.key.path}`}
              onClick={() => send({ type: "activate", id: tab.id })}
              onDoubleClick={() => send({ type: "pin", key: tab.key })}
              onKeyDown={(e) => {
                if (e.altKey && e.shiftKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
                  e.preventDefault();
                  move(tab.id, Math.max(0, index + (e.key === "ArrowRight" ? 1 : -1)));
                }
              }}
            >
              {tab.key.path.split("/").at(-1)}
            </button>
            <button type="button" aria-label={`${tab.key.path} 닫기`} onClick={() => void closeCodeTab(tab.key)}>
              ×
            </button>
          </div>
        ))}
      </div>
      {active ? (
        <>
          <div className={s.toolbar}>
            {!active.pinned && (
              <Button size="sm" onClick={() => send({ type: "pin", key: active.key })}>
                탭 고정
              </Button>
            )}
            <Button
              size="sm"
              onClick={() =>
                send({ type: "move", key: active.key, group: group === 0 ? 1 : 0, index: layout.tabs.length })
              }
            >
              다른 쪽으로 이동
            </Button>
            <Button
              size="sm"
              onClick={() =>
                void openPopout("code", keyId(active.key)).catch(() =>
                  useToasts.getState().push({ tone: "warn", title: "새 창을 열지 못했습니다." }),
                )
              }
            >
              새 창으로
            </Button>
          </div>
          <CodeEditor docKey={active.key} location={locations[keyId(active.key)]} />
        </>
      ) : (
        <EmptyState title="파일을 선택하세요" body="파일을 이쪽으로 끌어 편집기를 나눌 수 있습니다." />
      )}
    </section>
  );
}
