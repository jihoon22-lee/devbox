import { useState } from "react";
import "./App.css";
import { GROUPS, TOOLS } from "./tools";
import { SmartWorkflowPanel } from "./workflows/SmartWorkflowPanel";

export default function App() {
  const [activeId, setActiveId] = useState(TOOLS[0].id);
  const active = TOOLS.find((t) => t.id === activeId) ?? TOOLS[0];
  const ActiveComponent = active.component;

  return (
    <div className="app">
      <aside className="sidebar">
        <h1 className="app-title">Dev Toolbox</h1>
        {GROUPS.map((group) => (
          <div key={group} className="group">
            <div className="group-name">{group}</div>
            {TOOLS.filter((t) => t.group === group).map((tool) => (
              <button
                key={tool.id}
                className={`tool-link ${tool.id === activeId ? "active" : ""}`}
                onClick={() => setActiveId(tool.id)}
              >
                {tool.name}
              </button>
            ))}
          </div>
        ))}
      </aside>
      <main className="content">
        <SmartWorkflowPanel activeToolId={activeId} onOpenTool={setActiveId} />
        <h2 className="tool-title">{active.name}</h2>
        <ActiveComponent />
      </main>
    </div>
  );
}
