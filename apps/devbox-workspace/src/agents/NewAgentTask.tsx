import { useState } from "react";
import type { AgentTool } from "@devbox/workspace-features/generated/AgentTool";
export default function NewAgentTask({
  busy,
  submit,
}: {
  busy: boolean;
  submit(input: { title: string; tool: AgentTool; command?: string }): void;
}) {
  const [title, setTitle] = useState("");
  const [tool, setTool] = useState<AgentTool>("claudeCode");
  const [command, setCommand] = useState("");
  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        submit({ title, tool, ...(tool === "custom" ? { command } : {}) });
      }}
    >
      <fieldset disabled={busy}>
        <legend>새 에이전트 작업</legend>
        <label>
          제목
          <input value={title} maxLength={120} required onChange={(event) => setTitle(event.target.value)} />
        </label>
        <label>
          도구
          <select value={tool} onChange={(event) => setTool(event.target.value as AgentTool)}>
            <option value="claudeCode">Claude Code</option>
            <option value="codex">Codex</option>
            <option value="custom">직접 입력</option>
          </select>
        </label>
        {tool === "custom" && (
          <label>
            명령
            <input value={command} required onChange={(event) => setCommand(event.target.value)} />
          </label>
        )}
        <button type="submit">작업 만들기</button>
      </fieldset>
    </form>
  );
}
