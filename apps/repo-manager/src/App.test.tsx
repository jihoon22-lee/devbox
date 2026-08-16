import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import App from "./App";

// isTauri()가 false인 환경(MOCK, api.ts의 MOCK_RESULT)에서 렌더링해 검증한다.

afterEach(() => cleanup());

describe("App", () => {
  beforeEach(() => {
    render(<App />);
  });

  it("mock 탐색 결과의 repository를 표시한다", async () => {
    // repo-path span에서만 찾는다: 같은 경로가 자기 자신의 worktree 목록에도
    // "mono" span으로 다시 나타나 텍스트가 중복된다.
    await screen.findByText("C:\\projects\\devbox", { selector: ".repo-path" });
  });

  it("worktree가 2개 이상이면 worktree 목록을 보여준다", async () => {
    await screen.findByText("C:\\projects\\devbox-wt");
    expect(screen.getAllByText("remove 확인").length).toBeGreaterThan(0);
  });

  it("탐색이 잘리지 않았으면 truncated 배너를 보이지 않는다", async () => {
    await screen.findByText("C:\\projects\\devbox", { selector: ".repo-path" });
    expect(screen.queryByText(/일부 디렉터리를 건너뛰었습니다/)).toBeNull();
  });

  it("worktree remove는 clean 여부만 알려주고 실제 삭제는 하지 않는다", async () => {
    await screen.findByText("C:\\projects\\devbox-wt");
    screen.getAllByText("remove 확인")[0].click();
    await screen.findByText(/제거 가능 \(동작 미구현: remove는 신중히\)/);
  });
});
