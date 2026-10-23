/** 컬렉션 트리: 루트의 `*.request.json`을 경로 순서로 보인다. 열린 탭이 dirty면 점을 붙인다. */
import { useState } from "react";
import type { CollectionRoot } from "../../../rpc/gen/rpc";
import { messageFor } from "../../../rpc/messages";
import s from "./Api.module.css";
import { SendFailure, useApiBackend } from "./backend";
import { DirtyMark } from "./RequestTabs";
import { isDirty, tabKey, useApiTabs } from "./store";
import { isRequestPath, requestStem, useCollections } from "./useCollections";

export function CollectionTree({ root, label }: { root: CollectionRoot; label?: string }) {
  const backend = useApiBackend();
  const list = useCollections(root);
  const tabs = useApiTabs((state) => state.tabs);
  const active = useApiTabs((state) => state.active);
  const [error, setError] = useState<string>();
  const name = label ?? (root.kind === "personal" ? "개인 컬렉션" : "프로젝트 컬렉션");
  const items = list.items.filter((item) => isRequestPath(item.path)).sort((a, b) => a.path.localeCompare(b.path));
  const open = async (path: string) => {
    setError(undefined);
    try {
      const file = await backend.read(root, path);
      useApiTabs.getState().openFile(root, path, file.text, file.digest, file.reviewed);
    } catch (failure) {
      const code = failure instanceof SendFailure ? failure.code : "internal";
      setError(messageFor(code as never, failure instanceof SendFailure ? failure.detail : undefined)?.title);
    }
  };
  return (
    <div>
      <ul role="tree" aria-label={name} className={s.tree}>
        {items.map((item) => {
          const key = tabKey(root, item.path);
          const tab = tabs.find((t) => t.key === key);
          return (
            <li
              key={item.path}
              role="treeitem"
              aria-selected={active === key}
              tabIndex={0}
              className={s.treeItem}
              title={item.path}
              onClick={() => void open(item.path)}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  void open(item.path);
                }
              }}
            >
              {requestStem(item.path)}
              {item.unreadable && <span className={s.muted}> (읽을 수 없음)</span>}
              {!item.reviewed && <span className={s.muted}> (검토 필요)</span>}
              {tab && isDirty(tab) && <DirtyMark />}
            </li>
          );
        })}
      </ul>
      {list.loaded && items.length === 0 && !list.error && <p className={s.muted}>요청 파일이 없습니다.</p>}
      {list.truncated && <p className={s.muted}>파일이 많아 처음 2,000개만 보입니다.</p>}
      {(list.error || error) && (
        <p role="alert" className={s.muted}>
          {error ?? messageFor(list.error!.code as never, list.error!.detail)?.title}
        </p>
      )}
    </div>
  );
}
