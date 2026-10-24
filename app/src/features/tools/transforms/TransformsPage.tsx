import { useEffect, useState } from "react";
import type { ToolId } from "./catalog";
import { recentTools, rememberTool } from "./recent";
import { TOOLS } from "./registry";
import { ToolPicker } from "./ToolPicker";
import s from "./Transforms.module.css";
export function TransformsPage({ toolId, onSelect }: { toolId?: string; onSelect: (id: ToolId) => void }) {
  const tool = TOOLS.find((t) => t.id === toolId) ?? TOOLS[0]!;
  const Active = tool.component;
  const [recent, setRecent] = useState(recentTools);
  useEffect(() => {
    setRecent(rememberTool(tool.id));
  }, [tool.id]);
  return (
    <section className={s.page} aria-label="변환 도구">
      <ToolPicker selected={tool.id} recent={recent} onSelect={onSelect} />
      <h2>
        {tool.group} · {tool.name}
      </h2>
      <p className="tool-local-note">입력과 결과는 이 화면의 메모리에서만 처리합니다. 최근 도구 이름만 저장합니다.</p>
      <Active key={tool.id} />
    </section>
  );
}
