import { Columns2, Rows2, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useCommands } from "../../shell/commands";
import { type Binding, useKeymap } from "../../shell/keymap";
import { SectionLayout } from "../../shell/SectionLayout";
import { Button, EmptyState, IconButton, Tabs, useToasts } from "../../ui";
import { useCurrentProject } from "../projects/store";
import { useTerminalFocus } from "./focus";
import { PaneTree } from "./PaneTree";
import { SessionList } from "./SessionList";
import {
  closePane,
  EMPTY_LAYOUT,
  openTab,
  projectKey,
  prune,
  sessionsIn,
  setRatio,
  splitActive,
  titleOf,
  useTerminalLayout,
} from "./store";
import s from "./TerminalPage.module.css";
import { useCreateSession, useSessions } from "./useSessions";

export function TerminalPage() {
  const projectId = useCurrentProject((state) => state.id) ?? null;
  const project = projectKey(projectId);
  const sessions = useSessions(projectId);
  const layout = useTerminalLayout((state) => state.byProject[project] ?? EMPTY_LAYOUT);
  const update = useTerminalLayout((state) => state.update);
  const focusState = useTerminalFocus();
  const [focusRequest, setFocusRequest] = useState<{ session: string; version: number }>();
  const active = layout.tabs.find((tab) => tab.id === layout.active);
  const visible = active ? sessionsIn(active.root) : [];
  const lastFocused = focusState.byProject[project];
  const focused = lastFocused && visible.includes(lastFocused) ? lastFocused : visible[0];
  const create = useCreateSession();
  const pending = useRef(new Set<string>());
  const [, setCreationVersion] = useState(0);
  const creating = pending.current.has(project);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  useEffect(() => {
    if (!sessions.data || sessions.isError || sessions.isFetching || creating) return;
    update(project, (current) =>
      prune(current, new Set(sessions.items.filter((session) => session.alive).map((session) => session.id))),
    );
  }, [sessions.data, sessions.items, sessions.isError, sessions.isFetching, project, update, creating]);
  const newSession = async (place: "tab" | "row" | "column", command?: string, attachZellij?: string) => {
    if (pending.current.has(project)) return false;
    const origin = project,
      target = focused;
    pending.current.add(origin);
    setCreationVersion((version) => version + 1);
    try {
      const session = await create({
        projectId,
        cwd: target ? (focusState.cwd[target] ?? null) : null,
        title: null,
        command: command ?? null,
        attachZellij: attachZellij ?? null,
      });
      update(origin, (current) =>
        place === "tab" || !target ? openTab(current, session.id) : splitActive(current, target, session.id, place),
      );
      useTerminalFocus.getState().focus(origin, session.id);
      return true;
    } catch {
      useToasts.getState().push({ tone: "danger", title: "터미널을 만들지 못했습니다. 다시 시도하세요" });
      return false;
    } finally {
      pending.current.delete(origin);
      if (mounted.current) setCreationVersion((version) => version + 1);
    }
  };
  const closeFocused = () => {
    if (focused) update(project, (current) => closePane(current, focused));
  };
  const actions = useRef({ newSession, closeFocused });
  actions.current = { newSession, closeFocused };
  const bindings = useMemo<Binding[]>(
    () =>
      [
        { id: "term.new", code: "KeyT", label: "새 터미널", run: () => void actions.current.newSession("tab") },
        {
          id: "term.splitRight",
          code: "KeyE",
          label: "오른쪽 분할",
          run: () => void actions.current.newSession("row"),
        },
        {
          id: "term.splitDown",
          code: "KeyD",
          label: "아래 분할",
          run: () => void actions.current.newSession("column"),
        },
        { id: "term.closePane", code: "KeyW", label: "칸 닫기", run: () => actions.current.closeFocused() },
      ].map((binding) => ({
        ...binding,
        ctrl: true,
        shift: true,
        scope: "section",
        enabled: () => {
          const element = document.activeElement;
          return (
            !!element?.closest('[data-keyscope="terminal"]') ||
            !element?.closest('input, textarea, select, [contenteditable="true"]')
          );
        },
      })),
    [],
  );
  useKeymap(bindings);
  const register = useCommands((state) => state.register);
  useEffect(() => {
    const cleanups = bindings
      .filter((binding) => binding.id !== "term.new")
      .map((binding) =>
        register({
          id: binding.id,
          label: binding.label,
          keys: ["Ctrl", "Shift", binding.code.slice(3)],
          run: binding.run,
        }),
      );
    return () => cleanups.forEach((cleanup) => cleanup());
  }, [bindings, register]);
  const onOpen = (id: string) => {
    update(project, (current) => openTab(current, id));
    focusState.focus(project, id);
    setFocusRequest((current) => ({ session: id, version: (current?.version ?? 0) + 1 }));
  };
  return (
    <SectionLayout
      list={
        <SessionList
          sessions={sessions.items}
          projectTitle={projectId ? "프로젝트 터미널" : "프로젝트 없음"}
          loading={!sessions.data && !sessions.isError}
          failed={sessions.isError}
          disabled={creating}
          onRetry={() => void sessions.refetch()}
          onOpen={onOpen}
          onNew={(command, zellij) => newSession("tab", command, zellij)}
          onEnded={(id) => update(project, (current) => closePane(current, id))}
        />
      }
    >
      <div className={s.page} data-terminal-page="">
        {active ? (
          <>
            <Tabs
              items={layout.tabs.map((tab) => ({
                id: tab.id,
                label: titleOf(tab, sessions.items),
                closable: true,
                badge: sessionsIn(tab.root).some((id) => focusState.bells[id]) ? (
                  <span aria-label="터미널 벨"> ●</span>
                ) : undefined,
              }))}
              active={layout.active}
              onSelect={(id) => update(project, (current) => ({ ...current, active: id }))}
              onClose={(id) =>
                update(project, (current) => {
                  const tab = current.tabs.find((tab) => tab.id === id);
                  return tab ? sessionsIn(tab.root).reduce(closePane, current) : current;
                })
              }
              actions={
                <>
                  <IconButton
                    label="오른쪽 분할"
                    disabled={creating}
                    icon={<Columns2 size={16} />}
                    onClick={() => void newSession("row")}
                  />
                  <IconButton
                    label="아래 분할"
                    disabled={creating}
                    icon={<Rows2 size={16} />}
                    onClick={() => void newSession("column")}
                  />
                  <IconButton label="칸 닫기" icon={<X size={16} />} onClick={closeFocused} />
                </>
              }
            />
            <div className={s.body}>
              <PaneTree
                key={active.id}
                pane={active.root}
                focused={focused}
                focusRequest={focusRequest}
                onFocus={(id) => {
                  focusState.focus(project, id);
                  setFocusRequest((current) => (current && current.session !== id ? undefined : current));
                }}
                onCwd={focusState.setCwd}
                onRatio={(path, ratio) => update(project, (current) => setRatio(current, active.id, path, ratio))}
                onEnded={(id) => update(project, (current) => closePane(current, id))}
              />
            </div>
          </>
        ) : (
          <EmptyState
            title="열린 터미널이 없습니다"
            body="Ctrl+Shift+T로 새 세션을 엽니다. 앱을 닫아도 세션은 계속 실행됩니다."
            action={
              <Button variant="primary" disabled={creating} onClick={() => void newSession("tab")}>
                새 세션 열기
              </Button>
            }
          />
        )}
      </div>
    </SectionLayout>
  );
}
