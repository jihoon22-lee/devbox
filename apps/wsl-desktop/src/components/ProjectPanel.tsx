import type { GitStatus } from "../types";

// TODO(workbench): 프로젝트 목록과 git 상태는 Workbench의 ProjectProfile로 이관한다.
// (docs/product-opportunities.md §3.1, §15.2). Workbench 출시 전까지 여기서 유지한다.
interface Props {
  projects: GitStatus[];
  newPath: string;
  onNewPathChange: (v: string) => void;
  onAddPath: () => void;
  onRemovePath: (path: string) => void;
}

export default function ProjectPanel({ projects, newPath, onNewPathChange, onAddPath, onRemovePath }: Props) {
  return (
    <div className="dash-section">
      <h3 className="dash-subtitle">Projects</h3>
      <div className="project-add">
        <input
          placeholder="Add project path (C:\projects\devbox)"
          value={newPath}
          onChange={(e) => onNewPathChange(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") onAddPath();
          }}
        />
        <button className="btn" onClick={onAddPath}>
          Add
        </button>
      </div>
      <table>
        <thead>
          <tr>
            <th>PATH</th>
            <th>BRANCH</th>
            <th>CHANGES</th>
            <th>STATUS</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {projects.map((p) => (
            <tr key={p.path}>
              <td className="mono">{p.path}</td>
              <td>{p.branch}</td>
              <td>{p.changes}</td>
              <td>
                <span className={p.clean ? "tag-ok" : "tag-dirty"}>{p.clean ? "clean" : "dirty"}</span>
              </td>
              <td>
                <button className="mini" title="Remove" onClick={() => onRemovePath(p.path)}>
                  ✕
                </button>
              </td>
            </tr>
          ))}
          {projects.length === 0 && (
            <tr>
              <td colSpan={5} className="empty">
                No projects added
              </td>
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}
