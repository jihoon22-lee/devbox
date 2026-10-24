import type { AppMs, ProjectMs } from "../../rpc/gen/rpc";
import { duration } from "./dates";
export function ActivityTables({ apps, projects }: { apps: AppMs[]; projects: ProjectMs[] }) {
  return (
    <>
      <table aria-label="앱별 활동">
        <thead>
          <tr>
            <th>앱</th>
            <th>시간</th>
          </tr>
        </thead>
        <tbody>
          {apps.map((item) => (
            <tr key={item.app}>
              <td>{item.app}</td>
              <td>{duration(item.ms)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <table aria-label="프로젝트별 활동">
        <thead>
          <tr>
            <th>프로젝트</th>
            <th>시간</th>
          </tr>
        </thead>
        <tbody>
          {projects.map((item) => (
            <tr key={item.projectId}>
              <td>{item.projectId}</td>
              <td>{duration(item.ms)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </>
  );
}
