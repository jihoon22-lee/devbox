import type { UnitsResult, UserUnitInfo } from "../../rpc/gen/rpc";
import { Badge, Banner } from "../../ui";

/** The run-state wording for systemd's states, with systemd's own words beside it. */
export function unitStateLabel({
  loadState,
  activeState,
  subState,
}: Pick<UserUnitInfo, "loadState" | "activeState" | "subState">): string {
  // A mask is the user's own decision; only `error` and `bad-setting` say the file is broken.
  if (loadState === "masked") return "막아 둠 (masked)";
  if (loadState === "error" || loadState === "bad-setting") return `unit 파일 오류 (${loadState})`;
  if (loadState === "not-found") return "unit 파일 없음 (not-found)";
  if (loadState !== "loaded") return `${loadState} (${activeState}/${subState})`;
  const label =
    activeState === "failed"
      ? "실패"
      : activeState === "inactive"
        ? "중지됨"
        : activeState === "activating"
          ? subState === "auto-restart"
            ? "재시작 중"
            : "시작 중"
          : activeState === "deactivating"
            ? "중지 중"
            : activeState === "active"
              ? ({ running: "실행 중", waiting: "대기 중", exited: "종료" }[subState] ?? "켜짐")
              : undefined;
  return label ? `${label} (${activeState}/${subState})` : `${activeState}/${subState}`;
}
function nextRun(unit: UserUnitInfo) {
  const at = unit.nextElapseMs === undefined ? undefined : new Date(unit.nextElapseMs);
  if (!at || Number.isNaN(at.getTime())) return "—";
  return <time dateTime={at.toISOString()}>{at.toLocaleString("ko-KR")}</time>;
}
/** Read-only: the user's own systemd user units. Nothing here acts on a unit. */
export function UserUnits({ result, projectId }: { result: UnitsResult; projectId?: string }) {
  if (!result.available)
    return (
      <Banner tone="info">
        {result.reason === "cliMissing" ? (
          <>
            <code>systemctl</code>을 찾지 못해 사용자 서비스를 볼 수 없습니다. WSL 배포판에서 systemd를 켰는지
            확인하세요.
          </>
        ) : (
          <>
            systemd 사용자 관리자에 연결하지 못해 사용자 서비스를 볼 수 없습니다. 터미널에서{" "}
            <code>systemctl --user status</code>가 되는지 확인하세요.
          </>
        )}
      </Banner>
    );
  const mine = (unit: UserUnitInfo) => projectId !== undefined && unit.projectId === projectId;
  // The current project's units first; the listing's order is kept inside each group.
  const ordered = [...result.items].sort((a, b) => Number(mine(b)) - Number(mine(a)));
  return (
    <>
      <p role="note">
        Devbox가 시작하지 않은, <code>~/.config/systemd/user</code>의 서비스와 타이머를 보여 주기만 합니다.
        시작·중지·수정은 터미널에서 <code>systemctl --user</code>로 합니다.
      </p>
      {result.truncated && <p>서비스와 타이머가 많아 일부만 표시합니다.</p>}
      {ordered.length === 0 ? (
        <p>
          표시할 사용자 서비스가 없습니다. <code>~/.config/systemd/user</code>에 unit 파일을 두고{" "}
          <code>systemctl --user</code>로 시작하거나 켜 두면 여기에 나옵니다.
        </p>
      ) : (
        <>
          {projectId !== undefined && <p>작업 폴더가 현재 프로젝트 안인 서비스를 먼저 표시합니다.</p>}
          <table>
            <thead>
              <tr>
                <th>이름</th>
                <th>종류</th>
                <th>상태</th>
                <th>다음 실행</th>
                <th>설명</th>
              </tr>
            </thead>
            <tbody>
              {ordered.map((unit) => (
                <tr key={unit.name}>
                  <td>
                    {unit.name}
                    {mine(unit) && (
                      <>
                        {" "}
                        <Badge tone="info">현재 프로젝트</Badge>
                      </>
                    )}
                  </td>
                  <td>
                    {unit.kind === "timer" ? "타이머" : "서비스"}
                    {unit.triggers && ` → ${unit.triggers}`}
                  </td>
                  <td>{unitStateLabel(unit)}</td>
                  <td>{nextRun(unit)}</td>
                  <td>{unit.description || "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
    </>
  );
}
