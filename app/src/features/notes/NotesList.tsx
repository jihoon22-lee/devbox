import { CalendarDays, FilePlus } from "lucide-react";
import { useState } from "react";
import { useRpcSession } from "../../rpc/RpcProvider";
import { EmptyState, IconButton, Tabs } from "../../ui";
import { createNote, openDaily } from "./actions";
import { NoteTree } from "./NoteTree";
import s from "./notes.module.css";
import { TagList } from "./TagList";

const TABS = [
  { id: "tree", label: "폴더" },
  { id: "tags", label: "태그" },
  { id: "activity", label: "활동" },
];

/** The notes section list: folder tree, tags and the activity tab (filled by the activity views). */
export function NotesList() {
  const { client } = useRpcSession();
  const [tab, setTab] = useState("tree");
  return (
    <div className={s.list}>
      <Tabs
        items={TABS}
        active={tab}
        onSelect={setTab}
        label="노트 목록 보기"
        actions={
          <>
            <IconButton
              label="새 노트"
              shortcut={["Ctrl", "N"]}
              icon={<FilePlus size={16} />}
              onClick={() => void createNote(client)}
            />
            <IconButton
              label="일일 기록"
              shortcut={["Ctrl", "Alt", "D"]}
              icon={<CalendarDays size={16} />}
              onClick={() => void openDaily(client)}
            />
          </>
        }
      />
      <div className={s.listBody} role="tabpanel" aria-label={TABS.find((t) => t.id === tab)?.label}>
        {tab === "tree" && <NoteTree />}
        {tab === "tags" && <TagList />}
        {tab === "activity" && (
          <EmptyState title="활동 기록" body="활동 기록 보기는 다음 단계에서 이곳에 표시됩니다." />
        )}
      </div>
    </div>
  );
}
