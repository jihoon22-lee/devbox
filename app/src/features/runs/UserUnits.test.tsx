import { render, screen, within } from "@testing-library/react";
import type { UnitsResult, UserUnitInfo } from "../../rpc/gen/rpc";
import { UserUnits, unitStateLabel } from "./UserUnits";

const unit = (name: string, extra: Partial<UserUnitInfo> = {}): UserUnitInfo => ({
  name,
  description: `${name} 설명`,
  loadState: "loaded",
  activeState: "active",
  subState: "running",
  kind: name.endsWith(".timer") ? "timer" : "service",
  ...extra,
});
const listed = (items: UserUnitInfo[], truncated = false): UnitsResult => ({ available: true, items, truncated });
const shownNames = () =>
  screen
    .getAllByRole("row")
    .slice(1)
    .map((row) => within(row).getAllByRole("cell")[0]?.textContent);

test("the current project's units come first, marked, and every group keeps its order", () => {
  const items = [
    unit("app-backup.timer"),
    unit("app-web.service", { projectId: "p" }),
    unit("other-api.service", { projectId: "q" }),
    unit("app-worker.service", { projectId: "p" }),
  ];
  const { rerender } = render(<UserUnits result={listed(items)} projectId="p" />);
  expect(shownNames()).toEqual([
    "app-web.service 현재 프로젝트",
    "app-worker.service 현재 프로젝트",
    "app-backup.timer",
    "other-api.service",
  ]);
  expect(items.map((item) => item.name)).toEqual([
    "app-backup.timer",
    "app-web.service",
    "other-api.service",
    "app-worker.service",
  ]);
  expect(screen.getByText("app-web.service 설명")).toBeVisible();
  expect(
    screen.getByText("작업 폴더가 현재 프로젝트 안인 서비스와 그 서비스를 시작하는 타이머를 먼저 표시합니다."),
  ).toBeVisible();

  // Without a current project nothing is reordered or marked.
  rerender(<UserUnits result={listed(items)} />);
  expect(shownNames()).toEqual(["app-backup.timer", "app-web.service", "other-api.service", "app-worker.service"]);
  expect(screen.queryByText("현재 프로젝트")).not.toBeInTheDocument();
  expect(screen.queryByText(/먼저 표시합니다/)).not.toBeInTheDocument();
});

test("a timer shows its next run in local time and the unit it starts; a service shows neither", () => {
  const next = Date.UTC(2026, 0, 1, 3, 0, 0);
  render(
    <UserUnits
      result={listed([
        unit("app-backup.timer", { subState: "waiting", nextElapseMs: next, triggers: "app-backup.service" }),
        unit("app-health.timer", { subState: "waiting" }),
        unit("app-web.service"),
      ])}
    />,
  );
  const [, backup, health, web] = screen.getAllByRole("row");
  const time = within(backup!).getByText(new Date(next).toLocaleString("ko-KR"));
  expect(time.tagName).toBe("TIME");
  expect(time).toHaveAttribute("datetime", "2026-01-01T03:00:00.000Z");
  expect(within(backup!).getAllByRole("cell")[1]).toHaveTextContent("타이머 → app-backup.service");
  expect(within(backup!).getAllByRole("cell")[2]).toHaveTextContent("대기 중 (active/waiting)");
  // A timer with nothing scheduled has no time to show.
  expect(within(health!).getAllByRole("cell")[1]).toHaveTextContent(/^타이머$/);
  expect(within(health!).getAllByRole("cell")[3]).toHaveTextContent(/^—$/);
  expect(within(web!).getAllByRole("cell")[1]).toHaveTextContent(/^서비스$/);
  expect(within(web!).getAllByRole("cell")[3]).toHaveTextContent(/^—$/);
});

test("a timer of the current project is grouped with the service it starts", () => {
  render(
    <UserUnits
      result={listed([
        unit("app-backup.service", { projectId: "p", activeState: "inactive", subState: "dead" }),
        unit("app-backup.timer", { projectId: "p", subState: "waiting", triggers: "app-backup.service" }),
        unit("other.timer", { subState: "waiting" }),
      ])}
      projectId="p"
    />,
  );
  expect(shownNames()).toEqual(["app-backup.service 현재 프로젝트", "app-backup.timer 현재 프로젝트", "other.timer"]);
});

test.each([
  ["active", "running", "loaded", "실행 중 (active/running)"],
  ["active", "waiting", "loaded", "대기 중 (active/waiting)"],
  ["active", "exited", "loaded", "종료 (active/exited)"],
  ["active", "listening", "loaded", "켜짐 (active/listening)"],
  ["inactive", "dead", "loaded", "중지됨 (inactive/dead)"],
  ["failed", "failed", "loaded", "실패 (failed/failed)"],
  ["activating", "start", "loaded", "시작 중 (activating/start)"],
  ["activating", "auto-restart", "loaded", "재시작 중 (activating/auto-restart)"],
  ["deactivating", "stop-sigterm", "loaded", "중지 중 (deactivating/stop-sigterm)"],
  ["maintenance", "x", "loaded", "maintenance/x"],
  ["inactive", "dead", "bad-setting", "unit 파일 오류 (bad-setting)"],
  ["inactive", "dead", "error", "unit 파일 오류 (error)"],
  // A mask is the user's own decision, not a broken file.
  ["inactive", "dead", "masked", "막아 둠 (masked)"],
  ["inactive", "dead", "not-found", "unit 파일 없음 (not-found)"],
  ["inactive", "dead", "stub", "stub (inactive/dead)"],
])("state %s/%s of a %s unit reads %s", (activeState, subState, loadState, label) => {
  expect(unitStateLabel({ activeState, subState, loadState })).toBe(label);
});

test("nothing can be done to a unit here, and the screen says how they are managed", () => {
  render(<UserUnits result={listed([unit("app-web.service")])} />);
  expect(screen.queryAllByRole("button")).toHaveLength(0);
  expect(screen.queryByRole("link")).not.toBeInTheDocument();
  expect(screen.getByRole("note")).toHaveTextContent(
    "Devbox가 시작하지 않은, ~/.config/systemd/user의 서비스와 타이머를 보여 주기만 합니다. 사용자 관리자가 불러온 것(실행 중이거나 실패한 것, 켜 둔 것, 타이머처럼 다른 unit이 시작하는 것)만 나오고, 켜 두지도 시작하지도 않은 unit 파일은 나오지 않습니다. 시작·중지·수정은 터미널에서 systemctl --user로 합니다.",
  );
  expect(screen.queryByText(/일부만/)).not.toBeInTheDocument();
});

test("a cut list says so", () => {
  render(<UserUnits result={listed([unit("app-web.service")], true)} />);
  expect(screen.getByText("서비스와 타이머가 많아 일부만 표시합니다.")).toBeVisible();
});

test("an empty list says why and what to do", () => {
  render(<UserUnits result={listed([])} projectId="p" />);
  expect(screen.queryByRole("table")).not.toBeInTheDocument();
  expect(screen.getByText(/표시할 사용자 서비스가 없습니다\./)).toHaveTextContent(
    "표시할 사용자 서비스가 없습니다. ~/.config/systemd/user에 unit 파일을 두고 systemctl --user로 시작하거나 켜 두면 여기에 나옵니다.",
  );
});

test.each([
  ["cliMissing", "systemctl을 찾지 못해 사용자 서비스를 볼 수 없습니다. WSL 배포판에서 systemd를 켰는지 확인하세요."],
  [
    "managerUnavailable",
    "systemd 사용자 관리자에 연결하지 못해 사용자 서비스를 볼 수 없습니다. 터미널에서 systemctl --user status가 되는지 확인하세요.",
  ],
] as const)("an unavailable list (%s) says the cause and the next step", (reason, text) => {
  render(<UserUnits result={{ available: false, reason, items: [], truncated: false }} />);
  expect(screen.getByRole("status")).toHaveTextContent(text);
  expect(screen.queryByRole("table")).not.toBeInTheDocument();
  expect(screen.queryAllByRole("button")).toHaveLength(0);
});
