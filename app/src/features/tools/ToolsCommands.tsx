/** 명령 팔레트: 새 API 요청·가져오기·컬렉션 실행. */
import { useEffect, useRef } from "react";
import { useCommands } from "../../shell/commands";
import { useCurrentProject } from "../projects/store";
import { useApiTabs } from "./api/store";
import type { ToolsSearch } from "./search";

export function ToolsCommands({ navigate }: { navigate: (search: ToolsSearch) => void }) {
  const go = useRef(navigate);
  go.current = navigate;
  const register = useCommands((state) => state.register);
  useEffect(() => {
    const project = () => useCurrentProject.getState().id ?? undefined;
    const cleanups = [
      register({
        id: "tools.api.new",
        label: "새 API 요청",
        run: () => {
          const id = project();
          useApiTabs.getState().openNew(id ? { kind: "project", projectId: id } : { kind: "personal" });
          go.current({ view: "api" });
        },
      }),
      register({
        id: "tools.api.import",
        label: "API 요청 가져오기",
        run: () => go.current({ view: "api", tool: "import" }),
      }),
      register({
        id: "tools.api.run",
        label: "API 컬렉션 실행",
        run: () => go.current({ view: "api", tool: "runner", root: project() ?? "personal" }),
      }),
    ];
    return () => cleanups.forEach((cleanup) => cleanup());
  }, [register]);
  return null;
}
