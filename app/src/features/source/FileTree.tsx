import * as M from "@radix-ui/react-dropdown-menu";
import { useInfiniteQuery } from "@tanstack/react-query";
import { useState } from "react";
import { RpcError } from "../../rpc/client";
import type { FileEntry, FileTreeCursor } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
import { useRpcSession } from "../../rpc/RpcProvider";
import { Button } from "../../ui";
import s from "./source.module.css";
import { useSource } from "./store";
export type FileAction = "create" | "rename" | "delete" | "code" | "cursor" | "zed" | "terminal";
interface Props {
  projectId: string;
  showHidden: boolean;
  onAction(action: FileAction, entry: FileEntry): void;
}
export function FileTree(props: Props) {
  return <Folder {...props} dir="" />;
}
function Folder({ dir, ...props }: Props & { dir: string }) {
  const { client, state, generation } = useRpcSession();
  const query = useInfiniteQuery({
    queryKey: ["files.tree", { projectId: props.projectId, dir, showHidden: props.showHidden }],
    initialPageParam: null as FileTreeCursor | null,
    queryFn: ({ pageParam, signal }) =>
      client.call(
        "files.tree",
        { projectId: props.projectId, dir, showHidden: props.showHidden, cursor: pageParam },
        { signal },
      ),
    getNextPageParam: (page) => page.nextCursor ?? undefined,
    enabled: state === "ready",
    networkMode: "always",
    retry: false,
    meta: { generation },
  });
  const rows = query.data?.pages.flatMap((page) => page.entries) ?? [];
  return (
    <>
      {query.isPending && <p role="status">파일 불러오는 중…</p>}
      {query.error && (
        <p role="alert">
          {query.error instanceof RpcError
            ? messageFor(query.error.code, query.error.detail).title
            : "파일을 읽지 못했습니다."}
          <Button size="sm" onClick={() => void query.refetch()}>
            새로고침
          </Button>
        </p>
      )}
      <ul className={s.tree} role={dir ? "group" : "tree"} aria-label={dir || "프로젝트 파일"}>
        {rows.map((entry) => (
          <Row key={entry.name} entry={entry} {...props} />
        ))}
      </ul>
      {query.hasNextPage && (
        <Button size="sm" disabled={query.isFetchingNextPage} onClick={() => void query.fetchNextPage()}>
          파일 더 보기
        </Button>
      )}
      {!query.isPending && !rows.length && !query.error && <p>빈 폴더입니다.</p>}
    </>
  );
}
function Row({ entry, ...props }: Props & { entry: FileEntry }) {
  const [expanded, setExpanded] = useState(false);
  const [menu, setMenu] = useState(false);
  const directory = entry.kind === "directory";
  const open = (pinned = false) => {
    if (directory) setExpanded((value) => !value);
    else void useSource.getState().open(entry.key, { pinned });
  };
  const items: [FileAction, string][] = [
    ["create", "여기에 새 파일"],
    ["rename", "이름 바꾸기"],
    ["delete", "삭제"],
    ["terminal", "터미널에서 열기"],
    ...(!directory
      ? ([
          ["code", "VS Code로 열기"],
          ["cursor", "Cursor로 열기"],
          ["zed", "Zed로 열기"],
        ] as [FileAction, string][])
      : []),
  ];
  return (
    <li role="treeitem" aria-expanded={directory ? expanded : undefined}>
      <M.Root open={menu} onOpenChange={setMenu}>
        <div
          className={s.treeRow}
          onContextMenu={(event) => {
            event.preventDefault();
            setMenu(true);
          }}
        >
          <button
            type="button"
            onClick={() => open()}
            onDoubleClick={() => !directory && open(true)}
            onKeyDown={(e) => {
              if (directory && e.key === "ArrowRight") {
                e.preventDefault();
                setExpanded(true);
              }
              if (directory && e.key === "ArrowLeft") {
                e.preventDefault();
                setExpanded(false);
              }
              if (e.shiftKey && e.key === "F10") {
                e.preventDefault();
                setMenu(true);
              }
            }}
          >
            <span aria-hidden="true">
              {directory ? (expanded ? "▾ " : "▸ ") : entry.kind === "symlink" ? "↗ " : ""}
            </span>
            {entry.name}
          </button>
          <M.Trigger asChild>
            <button type="button" aria-label={`${entry.name} 작업`}>
              ⋯
            </button>
          </M.Trigger>
        </div>
        <M.Portal>
          <M.Content className={s.menu} sideOffset={4} align="start">
            {items.map(([action, label]) => (
              <M.Item
                key={action}
                className={s.menuItem}
                onSelect={() => queueMicrotask(() => props.onAction(action, entry))}
              >
                {label}
              </M.Item>
            ))}
          </M.Content>
        </M.Portal>
      </M.Root>
      {directory && expanded && <Folder dir={entry.key.path} {...props} />}
    </li>
  );
}
