import { useNavigate, useSearch } from "@tanstack/react-router";
import { SectionLayout } from "../../shell/SectionLayout";
import { Banner, Button, EmptyState, Kbd } from "../../ui";
import { AgentList } from "./AgentList";
import { AgentPreview } from "./AgentPreview";
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
                  <Button size="sm" onClick={() => void navigate({ to: "/settings" })}>
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
        <EmptyState
          title="에이전트 작업을 목록에서 확인하세요"
          body="목록에서 에이전트 작업을 선택해 확인하세요."
          action={
            <Button
              onClick={() =>
                void navigate({
                  to: "/agents",
                  search: (previous: Record<string, unknown>) => ({ ...previous, view: "list" }),
                })
              }
            >
              목록으로 이동
            </Button>
          }
        />
      ) : id ? (
        <AgentPreview key={id} id={id} />
      ) : (
        <EmptyState title="에이전트 작업을 고르세요" body="입력을 기다리는 작업이 위에 모입니다." />
      )}
    </SectionLayout>
  );
}
