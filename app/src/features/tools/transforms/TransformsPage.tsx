import { useEffect, useRef, useState } from "react";
import type { ToolId } from "./catalog";
import { TransformInputContext, useTransformHandoff } from "./handoff";
import { recentTools, rememberTool } from "./recent";
import { TOOLS } from "./registry";
import { ToolPicker } from "./ToolPicker";
import s from "./Transforms.module.css";
export function TransformsPage({ toolId, onSelect }: { toolId?: string; onSelect: (id: ToolId) => void }) {
  const pending = useTransformHandoff((state) => state.pending);
  const [received, setReceived] = useState(pending);
  useEffect(() => {
    if (!pending) return;
    setReceived(pending);
    if (useTransformHandoff.getState().pending?.id === pending.id) {
      useTransformHandoff.setState({ pending: null });
    }
  }, [pending]);
  const tool = TOOLS.find((t) => t.id === toolId) ?? TOOLS[0]!;
  const previousTool = useRef(tool.id);
  useEffect(() => {
    if (previousTool.current === tool.id) return;
    previousTool.current = tool.id;
    if (received?.toolId !== tool.id) setReceived(null);
  }, [tool.id, received]);
  const Active = tool.component;
  const [recent, setRecent] = useState(recentTools);
  useEffect(() => {
    setRecent(rememberTool(tool.id));
  }, [tool.id]);
  return (
    <section className={s.page} aria-label="변환 도구">
      <ToolPicker
        selected={tool.id}
        recent={recent}
        onSelect={(id) => {
          setReceived(null);
          onSelect(id);
        }}
      />
      <h2>
        {tool.group} · {tool.name}
      </h2>
      <p className="tool-local-note">입력과 결과는 이 화면의 메모리에서만 처리합니다. 최근 도구 이름만 저장합니다.</p>
      <TransformInputContext.Provider value={received?.toolId === tool.id ? received : null}>
        {received?.toolId === tool.id && tool.id === "uuid" && (
          <label>
            전달받은 UUID
            <textarea aria-label="전달받은 UUID" value={received.text} readOnly />
          </label>
        )}
        <Active key={`${tool.id}:${received?.toolId === tool.id ? received.id : ""}`} />
      </TransformInputContext.Provider>
    </section>
  );
}
