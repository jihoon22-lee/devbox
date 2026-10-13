import { useQueryClient } from "@tanstack/react-query";
import { MoreHorizontal } from "lucide-react";
import { useRef, useState } from "react";
import { confirmPolicy, useRpcMutation } from "../../rpc/query";
import { Button, ConfirmDialog, EmptyState, ErrorState, IconButton, Menu, StatusDot } from "../../ui";
import { useTerminalFocus } from "./focus";
import { NewSessionMenu } from "./NewSessionMenu";
import { openPopout } from "./popout";
import s from "./TerminalPage.module.css";
import type { Session } from "./useSessions";

export function SessionList({
  sessions,
  loading,
  failed,
  projectTitle,
  disabled,
  onRetry,
  onOpen,
  onNew,
  onEnded,
}: {
  sessions: Session[];
  loading: boolean;
  failed: boolean;
  projectTitle: string;
  disabled: boolean;
  onRetry: () => void;
  onOpen: (id: string) => void;
  onNew: (command?: string, attachZellij?: string) => Promise<boolean>;
  onEnded: (id: string) => void;
}) {
  const rename = useRpcMutation("terminal.rename");
  const close = useRpcMutation("terminal.close");
  const cache = useQueryClient();
  const bells = useTerminalFocus((state) => state.bells);
  const [editing, setEditing] = useState<string>();
  const [title, setTitle] = useState("");
  const [ending, setEnding] = useState<Session>();
  const [error, setError] = useState<string>();
  const busy = useRef(false);
  const save = async (id: string) => {
    if (busy.current || !title.trim()) return;
    busy.current = true;
    setError(undefined);
    try {
      await rename.mutateAsync({ id, title: title.trim() });
      setEditing(undefined);
      await cache.invalidateQueries({ queryKey: ["terminal.list"] });
    } catch {
      setError("이름을 바꾸지 못했습니다. 다시 시도하세요");
    } finally {
      busy.current = false;
    }
  };
  const end = async (session: Session) => {
    if (busy.current) return;
    busy.current = true;
    setEnding(undefined);
    setError(undefined);
    try {
      await close.mutateAsync({ id: session.id });
      onEnded(session.id);
      await cache.invalidateQueries({ queryKey: ["terminal.list"] });
    } catch {
      setError(`${session.title} 세션을 끝내지 못했습니다. 다시 시도하세요`);
    } finally {
      busy.current = false;
    }
  };
  return (
    <div className={s.list}>
      <header className={s.header}>
        <h2>{projectTitle}</h2>
        <Button disabled={disabled} onClick={() => void onNew()}>
          새 터미널
        </Button>
        <NewSessionMenu disabled={disabled} onNew={onNew} />
      </header>
      {error && (
        <p role="alert" className={s.error}>
          {error}
        </p>
      )}
      {failed ? (
        <ErrorState title="터미널 목록을 불러오지 못했습니다" action={<Button onClick={onRetry}>다시 시도</Button>} />
      ) : loading ? (
        <p role="status">터미널 목록을 불러오고 있습니다</p>
      ) : !sessions.length ? (
        <EmptyState title="터미널 세션이 없습니다" />
      ) : null}
      {sessions.map((session) => (
        <div className={s.row} key={session.id} data-terminal-list-session={session.id}>
          {editing === session.id ? (
            <form
              className={s.rename}
              onSubmit={(event) => {
                event.preventDefault();
                void save(session.id);
              }}
            >
              <input
                aria-label={`${session.title} 이름`}
                value={title}
                disabled={rename.isPending}
                autoFocus
                onChange={(event) => setTitle(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Escape") setEditing(undefined);
                }}
              />
              <Button size="sm" disabled={!title.trim() || rename.isPending} type="submit">
                저장
              </Button>
              <Button size="sm" onClick={() => setEditing(undefined)}>
                취소
              </Button>
            </form>
          ) : (
            <button type="button" className={s.session} disabled={!session.alive} onClick={() => onOpen(session.id)}>
              <StatusDot tone={session.alive ? "ok" : "idle"} label={session.alive ? "실행 중" : "종료됨"} />
              {bells[session.id] && <StatusDot tone="warn" label="터미널 벨" />}
              <span className={s.text}>
                <span>{session.title}</span>
                <span className={s.command}>{session.command}</span>
              </span>
            </button>
          )}
          <Menu
            trigger={
              <IconButton label={`${session.title} 메뉴`} onClick={() => {}} icon={<MoreHorizontal size={16} />} />
            }
            items={[
              {
                label: "이름 바꾸기",
                afterClose: true,
                disabled: rename.isPending || close.isPending,
                onSelect: () => {
                  setEditing(session.id);
                  setTitle(session.title);
                },
              },
              {
                label: "새 창으로",
                disabled: !session.alive,
                onSelect: () => {
                  void openPopout("terminal", session.id).catch(() =>
                    setError("새 창을 열지 못했습니다. 다시 시도하세요"),
                  );
                },
              },
              {
                label: "세션 끝내기",
                afterClose: true,
                danger: true,
                disabled: close.isPending,
                onSelect: () => {
                  if (confirmPolicy("terminal.close") === "always") setEnding(session);
                },
              },
            ]}
          />
        </div>
      ))}
      <ConfirmDialog
        open={!!ending}
        title={`터미널 세션 끝내기 · ${ending?.title ?? ""}`}
        body="이 세션에서 실행 중인 명령이 종료됩니다."
        confirmLabel="세션 끝내기"
        onCancel={() => setEnding(undefined)}
        onConfirm={() => {
          if (ending) void end(ending);
        }}
      />
    </div>
  );
}
