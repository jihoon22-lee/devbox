import { useNavigate } from "@tanstack/react-router";
import { Button } from "../../ui";
export type ObservationView = "system" | "ports" | "processes";
export const observationLabels = { system: "WSL 리소스", ports: "포트 목록", processes: "프로세스 목록" };
export function ObservationNavigation({ view }: { view?: ObservationView }) {
  const navigate = useNavigate();
  return (
    <nav aria-label="실행 관찰 화면">
      {view && <Button onClick={() => void navigate({ to: "/runs", search: {} })}>실행 목록</Button>}
      {(["ports", "processes", "system"] as const).map((target) => (
        <Button
          key={target}
          aria-current={target === view ? "page" : undefined}
          onClick={() => void navigate({ to: "/runs", search: { view: target } })}
        >
          {observationLabels[target]}
        </Button>
      ))}
    </nav>
  );
}
