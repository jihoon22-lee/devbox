import {afterEach,beforeEach,expect,it,vi} from "vitest";
import {cleanup,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {assertNoA11yViolations} from "@devbox/a11y/testing";
import LegacyImports from "./LegacyImports";
import {nativeCall} from "./native";
vi.mock("./native",()=>({nativeCall:vi.fn(),issueMessage:()=>"설정 보관 실패"}));
const call=vi.mocked(nativeCall);
afterEach(cleanup);
beforeEach(()=>{call.mockReset();});
const ready={id:"native-job",source:"code-pad",phase:"ready",snapshotId:"native-snapshot",issue:null,manifest:{files:[
  {name:"session.json",bytes:200,sha256:"fixture",records:2,issue:null},
  {name:"recovery.json",bytes:20,sha256:"fixture",records:null,issue:"unsupported-schema"},
],missing:["lsp/config.json"]}};
it("only prepares a selected native source and distinguishes unsupported data from imported records",async()=>{
  let state:unknown=null;
  call.mockImplementation(async(_component,method)=>{
    if(method==="prepare_legacy_snapshot"){state=ready;return ready;}
    if(method==="legacy_snapshot_job")return state;
    throw new Error("unexpected mutation");
  });
  const {container}=render(<LegacyImports/>);
  expect(call.mock.calls.some(([,method])=>method==="prepare_legacy_snapshot")).toBe(false);
  fireEvent.change(screen.getByRole("combobox"),{target:{value:"code-pad"}});
  fireEvent.click(screen.getByRole("button",{name:"설정 확인 및 보관"}));
  await screen.findByText("설정 보관이 완료되었습니다.");
  expect(call).toHaveBeenCalledWith("workspace.migration","prepare_legacy_snapshot",{source:"code-pad"});
  expect(screen.getByText("2개 항목")).toBeTruthy();expect(screen.getByText("지원하지 않는 형식 — 원본 보관")).toBeTruthy();
  expect(screen.getByText("저장 파일 없음: 언어 서버 설정")).toBeTruthy();
  expect(call.mock.calls.every(([,method])=>["prepare_legacy_snapshot","legacy_snapshot_job"].includes(method))).toBe(true);
  await assertNoA11yViolations(container);
});
it("joins an existing job and waits for native cancellation before reporting it",async()=>{
  let state={...ready,phase:"preserving",manifest:null,snapshotId:null};
  call.mockImplementation(async(_component,method)=>{
    if(method==="cancel_legacy_snapshot")state={...state,phase:"cancelled"};
    return state;
  });
  render(<LegacyImports/>);
  fireEvent.click(await screen.findByRole("button",{name:"보관 취소"}));
  await screen.findByText(/보관을 취소했습니다/);
  expect(call).toHaveBeenCalledWith("workspace.migration","cancel_legacy_snapshot",{jobId:"native-job"});
  expect(screen.queryByText("설정 보관이 완료되었습니다.")).toBeNull();
});
it("a late initial status cannot replace the job explicitly started by the user",async()=>{
  let resolveInitial!:(value:unknown)=>void;
  let reads=0;
  call.mockImplementation(async(_component,method)=>method==="legacy_snapshot_job"&&++reads===1?new Promise(resolve=>{resolveInitial=resolve;}):ready);
  render(<LegacyImports/>);
  fireEvent.click(screen.getByRole("button",{name:"설정 확인 및 보관"}));
  await screen.findByText("설정 보관이 완료되었습니다.");
  resolveInitial(null);
  await waitFor(()=>expect(screen.getByText("설정 보관이 완료되었습니다.")).toBeTruthy());
});
