/** 명령 팔레트: 새 API 요청·가져오기·컬렉션 실행. */
import { useEffect, useRef } from "react";
import { useCommands } from "../../shell/commands";
import { useToasts } from "../../ui";
import { useCurrentProject } from "../projects/store";
import { useApiTabs } from "./api/store";
import type { ToolsSearch } from "./search";
import { claimTransformIntent, registerTransformsNavigation, sendToTransforms } from "./transforms/handoff";

export function ToolsCommands({ navigate }: { navigate: (search: ToolsSearch) => void | Promise<unknown> }) {
  const go = useRef(navigate);
  go.current = navigate;
  const register = useCommands((state) => state.register);
  useEffect(() => {
    let active = true;
    const unregisterNavigation = registerTransformsNavigation((search) => go.current(search));
    const project = () => useCurrentProject.getState().id ?? undefined;
    const cleanups = [
      register({
        id: "tools.clipboard",
        label: "클립보드로 변환…",
        run: () => {
          if (!active) return;
          const ownsIntent = claimTransformIntent();
          void Promise.resolve()
            .then(() => {
              if (active && ownsIntent()) return navigator.clipboard.readText();
            })
            .then((text) => {
              if (active && ownsIntent() && text !== undefined) return sendToTransforms(text, "clipboard");
            })
            .catch(() => {
              if (active && ownsIntent())
                useToasts
                  .getState()
                  .push({ tone: "warn", title: "클립보드를 읽지 못했습니다. 접근 권한을 확인해 주세요." });
            });
        },
      }),
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
    return () => {
      active = false;
      unregisterNavigation();
      cleanups.forEach((cleanup) => cleanup());
    };
  }, [register]);
  return null;
}
