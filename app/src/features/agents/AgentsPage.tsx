import { useNavigate, useSearch } from "@tanstack/react-router";
import { SectionLayout } from "../../shell/SectionLayout";
import { Banner, Button, Kbd } from "../../ui";
import { openSettings } from "../settings/open";
import { AgentEmpty } from "./AgentEmpty";
import { AgentGrid } from "./AgentGrid";
import { AgentList } from "./AgentList";
import { AgentDetail } from "./detail/AgentDetail";
import { useAgentsData } from "./hooks";
import { useAgentsUi } from "./ui";
export function agentsSearch(search: Record<string, unknown>) {
  return {
    id: typeof search.id === "string" && search.id.trim() ? search.id : undefined,
    view: search.view === "grid" ? "grid" : "list",
  };
}
export function AgentsPage() {
  const { id, view } = useSearch({ strict: false }) as ReturnType<typeof agentsSearch>;
  const navigate = useNavigate();
  const data = useAgentsData();
  const setCreating = useAgentsUi((state) => state.setCreating);
  const select = (id: string) =>
    void navigate({
      to: "/agents",
      search: (previous: Record<string, unknown>) => ({ ...previous, id, view: "list" }),
    });
  return (
    <SectionLayout
      list={
        <AgentList
          data={data}
          selected={id}
          onSelect={select}
          header={
            <>
              <Button
                size="sm"
                aria-pressed={view === "list"}
                onClick={() =>
                  void navigate({
                    to: "/agents",
                    search: (previous: Record<string, unknown>) => ({ ...previous, view: "list" }),
                  })
                }
              >
                목록
              </Button>
              <Button
                size="sm"
                aria-pressed={view === "grid"}
                onClick={() =>
                  void navigate({
                    to: "/agents",
                    search: (previous: Record<string, unknown>) => ({ ...previous, view: "grid" }),
                  })
                }
              >
                격자
              </Button>
              <Button size="sm" variant="primary" onClick={() => setCreating(true)}>
                새 작업 <Kbd keys={["Ctrl", "Shift", "N"]} />
              </Button>
            </>
          }
          banner={
            data.hookMissing && (
              <Banner
                tone="warn"
                actions={
                  <Button size="sm" onClick={() => openSettings("agents")}>
                    연결 확인
                  </Button>
                }
              >
                상태 알림이 연결되지 않았습니다
              </Banner>
            )
          }
        />
      }
    >
      {view === "grid" ? (
        <AgentGrid tasks={data.groups.flatMap((group) => group.items)} onOpen={select} />
      ) : id ? (
        <AgentDetail key={id} id={id} data={data} />
      ) : (
        <AgentEmpty onOpen={select} />
      )}
    </SectionLayout>
  );
}
