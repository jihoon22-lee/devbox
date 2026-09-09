import {afterEach,beforeEach,expect,it,vi} from "vitest";
import {cleanup,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {assertNoA11yViolations} from "@devbox/a11y/testing";
import LegacyImports from "./LegacyImports";
import {nativeCall} from "./native";
vi.mock("./native",()=>({nativeCall:vi.fn(),issueMessage:()=>"설정 보관 실패"}));
const call=vi.mocked(nativeCall);
afterEach(cleanup);
beforeEach(()=>{call.mockReset();});
const emptyCatalog={snapshots:[],unrecognized:0};
const ready={operation:"preserve",id:"native-job",source:"code-pad",phase:"ready",snapshotId:"native-snapshot",issue:null,manifest:{files:[
  {name:"session.json",bytes:200,sha256:"fixture",records:2,issue:null},
  {name:"recovery.json",bytes:20,sha256:"fixture",records:null,issue:"unsupported-schema"},
],missing:["lsp/config.json"]}};
it.each(["code-pad","code-pad-legacy"])("only prepares selected source %s and distinguishes unsupported data",async(source)=>{
  let state:unknown=null;
  call.mockImplementation(async(_component,method)=>{
    if(method==="list_legacy_snapshots")return emptyCatalog;
    if(method==="prepare_legacy_snapshot"){state={...ready,source};return state;}
    if(method==="legacy_snapshot_job")return state;
    throw new Error("unexpected mutation");
  });
  const {container}=render(<LegacyImports/>);
  expect(call.mock.calls.some(([,method])=>method==="prepare_legacy_snapshot")).toBe(false);
  fireEvent.change(screen.getByRole("combobox"),{target:{value:source}});
  fireEvent.click(screen.getByRole("button",{name:"설정 확인 및 보관"}));
  await screen.findByText("설정 보관이 완료되었습니다.");
  expect(call).toHaveBeenCalledWith("workspace.migration","prepare_legacy_snapshot",{source});
  expect(screen.getByText("2개 항목")).toBeTruthy();expect(screen.getByText("지원하지 않는 형식 — 원본 보관")).toBeTruthy();
  expect(screen.getByText("저장 파일 없음: 언어 서버 설정")).toBeTruthy();
  expect(call.mock.calls.every(([,method])=>["prepare_legacy_snapshot","legacy_snapshot_job","list_legacy_snapshots"].includes(method))).toBe(true);
  await assertNoA11yViolations(container);
});
it("joins an existing job and waits for native cancellation before reporting it",async()=>{
  let state={...ready,phase:"preserving",manifest:null,snapshotId:null};
  call.mockImplementation(async(_component,method)=>{
    if(method==="list_legacy_snapshots")return emptyCatalog;
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
  call.mockImplementation(async(_component,method)=>method==="list_legacy_snapshots"?emptyCatalog:method==="legacy_snapshot_job"&&++reads===1?new Promise(resolve=>{resolveInitial=resolve;}):ready);
  render(<LegacyImports/>);
  fireEvent.click(screen.getByRole("button",{name:"설정 확인 및 보관"}));
  await screen.findByText("설정 보관이 완료되었습니다.");
  resolveInitial(null);
  await waitFor(()=>expect(screen.getByText("설정 보관이 완료되었습니다.")).toBeTruthy());
});

it("reopens a saved snapshot by opaque ID and reports changed contents instead of trusting its catalog",async()=>{
  let state:unknown=null;
  call.mockImplementation(async(_component,method)=>{
    if(method==="list_legacy_snapshots")return {snapshots:[
      {id:"saved-id",manifest:{...ready.manifest,source:"code-pad"},issue:null},
      {id:"incomplete-id",manifest:null,issue:"legacy_snapshot_incomplete"},
    ],unrecognized:1};
    if(method==="legacy_snapshot_job")return state;
    if(method==="verify_legacy_snapshot"){state={...ready,operation:"verify",phase:"failed",issue:"legacy_snapshot_changed",manifest:null};return state;}
    throw new Error("unexpected mutation");
  });
  render(<LegacyImports/>);
  const verify=await screen.findByRole("button",{name:"보관 1 내용 다시 확인"});
  expect(screen.getByRole("button",{name:"보관 2 내용 다시 확인"}).hasAttribute("disabled")).toBe(true);
  expect(screen.queryByText("보관 파일 확인이 완료되었습니다.")).toBeNull();
  fireEvent.click(verify);
  await waitFor(()=>expect(call).toHaveBeenCalledWith("workspace.migration","verify_legacy_snapshot",{snapshotId:"saved-id"}));
  await waitFor(()=>expect(screen.getAllByText("설정 보관 실패").length).toBeGreaterThan(0));
  expect(screen.queryByText("보관 파일 확인이 완료되었습니다.")).toBeNull();
  expect(call.mock.calls.some(([,method])=>method==="prepare_legacy_snapshot")).toBe(false);
});
