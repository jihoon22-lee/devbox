import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { fixtureDescription } from "@devbox/product-shell/api";
import DevelopmentSessions from "./DevelopmentSessions";
import { componentCall } from "./native";

vi.mock("./native", () => ({ componentCall: vi.fn() }));
const call = vi.mocked(componentCall);
const context = { projectId: "project", worktreeId: "first", target: { kind: "windows" as const }, revision: 1 };
const description = { ...fixtureDescription("workspace"), context };
const session = { id: "10000000-0000-4000-8000-000000000001", context, revision: 2, planRevision: "a".repeat(64), phase: "review", issue: null };
const job = { id: "20000000-0000-4000-8000-000000000001", name: "개발 서버", kind: "service", targetKind: "windows", targetDistro: null, command: "synthetic-server", cwd: null, envConfigured: true };
const preflight={definitionsRevision:"d".repeat(64),restoreBlocked:false,executionBlocked:false,entries:[]};
beforeEach(() => {
  sessionStorage.clear();
  call.mockReset().mockImplementation(async (_description, _component, method) => {
    if (method === "development_candidates") return { jobs: [job], truncated: false };
    if (method === "development_sessions") return { sessions: [], intents: {} };
    if (method === "prepare_development_session") return { session, jobs: [job], preflight };
    return {};
  });
});
afterEach(cleanup);

it("previews commands without starting them and keeps restore-only approval separate", async () => {
  render(<DevelopmentSessions description={description} registry={null} />);
  fireEvent.click(await screen.findByRole("checkbox"));
  fireEvent.click(screen.getByRole("button", { name: "환경 확인·실행 검토" }));
  await screen.findByText("synthetic-server");
  expect(call.mock.calls.some(([, , method]) => method === "start_development_session")).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "상태만 이어가기" }));
  await waitFor(() => expect(call.mock.calls.find(([, , method]) => method === "start_development_session")?.[3]).toEqual({ id: session.id, revision: 2, planRevision: session.planRevision, mode: "restoreOnly" }));
});

it("does not display a late plan from the previous worktree", async () => {
  let finish: ((value: unknown) => void) | undefined;
  call.mockImplementation(async (_description, _component, method) => {
    if (method === "development_candidates") return { jobs: [job], truncated: false };
    if (method === "development_sessions") return { sessions: [], intents: {} };
    if (method === "prepare_development_session") return new Promise(resolve => { finish = resolve; });
    return {};
  });
  const view = render(<DevelopmentSessions description={description} registry={null} />);
  await screen.findByRole("checkbox");
  fireEvent.click(screen.getByRole("button", { name: "환경 확인·실행 검토" }));
  await waitFor(() => expect(finish).toBeDefined());
  view.rerender(<DevelopmentSessions description={{ ...description, context: { ...context, worktreeId: "second" } }} registry={null} />);
  finish?.({ session, jobs: [job], preflight });
  await waitFor(() => expect((screen.getByRole("button", { name: "환경 확인·실행 검토" }) as HTMLButtonElement).disabled).toBe(false));
  expect(screen.queryByText("synthetic-server")).toBeNull();
});

it("keeps state restoration available while a foreign listener blocks execution",async()=>{
  call.mockImplementation(async(_description,_component,method)=>{
    if(method==="development_candidates")return{jobs:[job],truncated:false};
    if(method==="development_sessions")return{sessions:[],intents:{}};
    if(method==="prepare_development_session")return{session,jobs:[job],preflight:{...preflight,executionBlocked:true,entries:[{kind:"port",key:"8080",state:"occupied",blocking:true}]}};
    return{};
  });
  render(<DevelopmentSessions description={description} registry={null}/>);
  await screen.findByRole("checkbox");
  fireEvent.click(screen.getByRole("button",{name:"환경 확인·실행 검토"}));
  await screen.findByText(/다른 실행이 사용 중/);
  expect((screen.getByRole("button",{name:"검토한 작업 실행"}) as HTMLButtonElement).disabled).toBe(true);
  expect((screen.getByRole("button",{name:"상태만 이어가기"}) as HTMLButtonElement).disabled).toBe(false);
  expect(call.mock.calls.some(([, , method])=>method==="start_development_session")).toBe(false);
});

it("shows a stopped-target report without creating or approving a session",async()=>{
  call.mockImplementation(async(_description,_component,method)=>{
    if(method==="development_candidates")return{jobs:[job],truncated:false};
    if(method==="development_sessions")return{sessions:[],intents:{}};
    if(method==="prepare_development_session")return{session:null,jobs:[job],preflight:{...preflight,restoreBlocked:true,executionBlocked:true,entries:[{kind:"distro",key:"synthetic",state:"stopped",blocking:true}]}};
    return{};
  });
  render(<DevelopmentSessions description={description} registry={null}/>);
  await screen.findByRole("checkbox");
  fireEvent.click(screen.getByRole("button",{name:"환경 확인·실행 검토"}));
  await screen.findByText(/중지된 WSL 배포판/);
  expect((screen.getByRole("button",{name:"상태만 이어가기"}) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole("button",{name:"취소"}));
  expect(call.mock.calls.some(([, , method])=>["start_development_session","stop_development_session"].includes(method))).toBe(false);
});

it("previews native summary metadata and reuses the operation receipt",async()=>{
  const requests:Array<Record<string,unknown>>=[];
  call.mockImplementation(async(_description,_component,method,args)=>{
    if(method==="development_candidates")return{jobs:[],truncated:false};
    if(method==="development_sessions")return{sessions:[session],intents:{}};
    if(method==="prepare_session_summary"){
      requests.push(args);
      return{operationId:args.operationId,draft:{title:"개발 세션 요약",body:"실패한 실행: 확인 불가",metadata:{binding:{context,sessionId:session.id,revision:session.revision}}}};
    }
    return{};
  });
  render(<DevelopmentSessions description={description} registry={null}/>);
  fireEvent.click(await screen.findByRole("button",{name:"요약 미리보기"}));
  await screen.findByText("실패한 실행: 확인 불가");
  fireEvent.click(screen.getByRole("button",{name:"미리보기 닫기"}));
  fireEvent.click(screen.getByRole("button",{name:"요약 미리보기"}));
  await waitFor(()=>expect(requests).toHaveLength(2));
  expect(requests[0].operationId).toBe(requests[1].operationId);
  expect(requests[0]).toEqual(expect.objectContaining({sessionId:session.id,revision:session.revision,includeProblems:false}));
});
