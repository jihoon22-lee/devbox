import { RPC_ERROR_CODES } from "./gen/rpc";
import { messageFor } from "./messages";

test("all generated codes have Korean messages and uncertain results stay explicit", () => {
  for (const code of RPC_ERROR_CODES) expect(messageFor(code).title).toMatch(/[가-힣]/);
  expect(messageFor("connection_lost").title).toContain("결과");
  expect(messageFor("projects.path_invalid", { path: "/hello" }).title).toContain("/hello");
  expect(messageFor("cancelled").title).not.toContain("되돌");
});

test("agent errors use actual generated file details and terminal codes", () => {
  expect(messageFor("agents.base_dirty", { files: ["src/main.rs"] }).action).toContain("src/main.rs");
  expect(messageFor("agents.conflict", { files: ["app.tsx"] }).action).toContain("app.tsx");
  expect(messageFor("agents.conflict", { files: [1, null] }).action).not.toContain("null");
  expect(messageFor("terminal.cwd_invalid").action).toContain("WSL");
  expect(messageFor("terminal.title_too_long", { limit: 256 }).action).toContain("256");
  // A send that came before the agent reads its input says when to send it again.
  expect(messageFor("agents.tool_starting")).toEqual({
    title: "에이전트가 시작하는 중입니다.",
    action: "터미널에 에이전트 화면이 나타나면 다시 보내세요.",
  });
  for (const code of ["agents.tool_missing", "projects.launch_tool_missing"] as const) {
    expect(messageFor(code, { command: "pi" })).toEqual({
      title: "pi 명령을 찾을 수 없습니다.",
      action: "설정 → 환경 점검에서 설치 방법을 확인하세요.",
    });
  }
});

test("an unready Windows job holder says to run again and where to look", () => {
  expect(messageFor("windows.observer_unavailable")).toEqual({
    title: "Windows 작업 관찰을 준비하지 못했습니다.",
    action: "다시 실행하세요. 계속되면 설정 → 앱 진단을 확인하세요.",
  });
});

test("a busy Windows copy names a live run and leftover build servers, and how to end them", () => {
  const busy = messageFor("runtime.windows_copy_busy");
  // The Runs page banner shows the title alone, so the title names both causes by itself.
  expect(busy).toEqual({
    title: "Windows 사본을 쓰는 실행이나 그 실행이 남긴 프로세스(빌드 서버 등)가 아직 끝나지 않았습니다.",
    action:
      "끝날 때까지 기다린 뒤 다시 실행하세요. 빌드 서버(dotnet build의 VBCSCompiler·MSBuild, Gradle 등)는 한동안 쓰이지 않아야 스스로 끝나므로, 바로 끝내려면 dotnet build-server shutdown이나 gradle --stop을 실행하거나 작업 관리자에서 끝내세요. 모두 끝났는데도 계속되면 Windows 앱 연결을 확인해 주세요.",
  });
  // "Gradle daemon" stays a build server: 데몬 is the devbox daemon on diagnostics screens only.
  expect(`${busy.title} ${busy.action}`).not.toMatch(/데몬|영수증|receipt/);
});

test("redaction failure actions distinguish capacity and rejected server input", () => {
  expect(messageFor("api.redaction_failed", { reason: "capacity" }).action).toContain("크기");
  expect(messageFor("api.redaction_failed", { reason: "input" }).action).toContain("서버 응답");
  expect(messageFor("api.redaction_failed", { reason: "matcher" }).action).toContain("다시 시도");
  expect(messageFor("api.redaction_failed", { reason: "fixture-sensitive-detail" }).action).not.toContain(
    "fixture-sensitive-detail",
  );
});

test("response export distinguishes pending history from unavailable provenance without echoing details", () => {
  expect(messageFor("notes.response_unavailable", { reason: "pending" }).title).toContain("저장");
  expect(messageFor("notes.response_unavailable", { reason: "provenance" }).action).toContain("이전 세션");
  expect(messageFor("notes.append_uncertain").action).toContain("같은 작업");
});

test("no message attaches the wrong particle to 파일", () => {
  const detail = { file: "/f/config.toml", path: "/p", files: ["a.rs"], names: ["N"], message: "m", index: 0 };
  for (const code of [...RPC_ERROR_CODES, "connection_lost"] as const)
    for (const message of [messageFor(code), messageFor(code, detail)])
      expect(`${code}: ${message.title} ${message.action ?? ""}`).not.toMatch(/파일를|파일가/);
});

test("settings-file errors name the file and where to recover", () => {
  expect(messageFor("secrets.config_invalid", { file: "/home/me/.config/devbox/config.toml" }).action).toContain(
    "/home/me/.config/devbox/config.toml",
  );
  for (const code of ["notes.disabled", "search.disabled", "windows.config_invalid"] as const) {
    expect(messageFor(code).action).toContain("설정 파일 형식");
    expect(messageFor(code).action).toContain("설정 → 앱 진단 → [다시 설정]");
  }
});
