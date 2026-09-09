import {afterEach,beforeEach,expect,it,vi} from "vitest";
import {cleanup,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {assertNoA11yViolations} from "@devbox/a11y/testing";
import LegacyWindowImport from "./LegacyWindowImport";
import {nativeCall} from "./native";
vi.mock("./native",()=>({nativeCall:vi.fn(),issueMessage:(issue:string)=>issue}));
const call=vi.mocked(nativeCall),onBusy=vi.fn();
const before={schemaVersion:1,bounds:{x:10,y:20,width:1180,height:780},monitorId:"native",monitorWorkArea:{x:0,y:0,width:1920,height:1080},scaleFactor:1,maximized:false};
const after={...before,bounds:{x:100,y:80,width:1000,height:700}};
const preview={previewId:"native-token",before,after,source:after,restoring:false};
afterEach(cleanup);beforeEach(()=>{call.mockReset();onBusy.mockReset();call.mockImplementation(async(_component,method)=>method==="list_window_history"?{items:[],unrecognized:0}:method.startsWith("preview_")?preview:null);});
it("requires native review and explicit replacement before applying only its token",async()=>{
  const {container}=render(<LegacyWindowImport jobId="verified-job" onBusyChange={onBusy}/>);
  await waitFor(()=>expect(call).toHaveBeenCalledWith("workspace.migration","list_window_history",{}));
  expect(call.mock.calls.some(([,method])=>method==="preview_window_import")).toBe(false);
  fireEvent.click(screen.getByRole("button",{name:"기존 창 상태 검토"}));
  const button=await screen.findByRole("button",{name:"창 상태 적용"});
  expect(button.hasAttribute("disabled")).toBe(true);
  expect(call).toHaveBeenCalledWith("workspace.migration","preview_window_import",{jobId:"verified-job"});
  await waitFor(()=>expect(screen.getByRole("checkbox").hasAttribute("disabled")).toBe(false));
  fireEvent.click(screen.getByRole("checkbox",{name:"현재 창 위치와 크기 변경"}));
  await assertNoA11yViolations(container);
  fireEvent.click(button);
  await screen.findByText("검토한 창 위치와 크기를 적용했습니다.");
  expect(call).toHaveBeenCalledWith("workspace.migration","apply_window_import",{previewId:"native-token",replaceExisting:true});
});
it("keeps restore available without a current legacy job and reports failed apply without success",async()=>{
  call.mockImplementation(async(_component,method)=>{
    if(method==="list_window_history")return {items:[{id:"native-backup",state:before,issue:null},{id:"bad",state:null,issue:"files_store_changed"}],unrecognized:1};
    if(method==="preview_window_restore")return {...preview,restoring:true};
    if(method==="apply_window_import")throw new Error("창 상태를 다시 검토해 주세요.");return null;
  });
  render(<LegacyWindowImport onBusyChange={onBusy}/>);
  fireEvent.click(await screen.findByRole("button",{name:"이전 창 상태 1 복원 검토"}));
  expect(screen.getByRole("button",{name:"이전 창 상태 2 복원 검토"}).hasAttribute("disabled")).toBe(true);
  await screen.findByText("복원할 창");await waitFor(()=>expect(screen.getByRole("checkbox").hasAttribute("disabled")).toBe(false));
  fireEvent.click(screen.getByRole("checkbox"));fireEvent.click(screen.getByRole("button",{name:"창 상태 적용"}));
  await screen.findByRole("alert");expect(screen.queryByText("검토한 창 위치와 크기를 적용했습니다.")).toBeNull();
  expect(call).toHaveBeenCalledWith("workspace.migration","preview_window_restore",{backupId:"native-backup"});
});
it("cancels late review tokens after a job changes and releases parent busy state",async()=>{
  let resolve!:(value:unknown)=>void;
  call.mockImplementation(async(_component,method)=>method==="preview_window_import"?new Promise(done=>{resolve=done;}):method==="list_window_history"?{items:[],unrecognized:0}:null);
  const {rerender}=render(<LegacyWindowImport jobId="old" onBusyChange={onBusy}/>);
  fireEvent.click(screen.getByRole("button",{name:"기존 창 상태 검토"}));
  rerender(<LegacyWindowImport jobId="new" onBusyChange={onBusy}/>);resolve(preview);
  await waitFor(()=>expect(call).toHaveBeenCalledWith("workspace.migration","cancel_window_import",{previewId:"native-token"}));
  expect(screen.queryByRole("button",{name:"창 상태 적용"})).toBeNull();
  expect(screen.getByRole("button",{name:"기존 창 상태 검토"}).hasAttribute("disabled")).toBe(false);
});

it("allows retrying a failed history load while preserving a pending review",async()=>{
  let reads=0;
  call.mockImplementation(async(_component,method)=>{
    if(method==="list_window_history"){if(++reads===1)throw new Error("기록 읽기 실패");return {items:[],unrecognized:0};}
    return preview;
  });
  render(<LegacyWindowImport jobId="job" onBusyChange={onBusy}/>);
  await screen.findByText("기록 읽기 실패");
  fireEvent.click(screen.getByRole("button",{name:"이전 창 상태 새로 고침"}));
  await waitFor(()=>expect(screen.queryByRole("alert")).toBeNull());
  await waitFor(()=>expect(screen.getByRole("button",{name:"기존 창 상태 검토"}).hasAttribute("disabled")).toBe(false));
  fireEvent.click(screen.getByRole("button",{name:"기존 창 상태 검토"}));
  await screen.findByRole("button",{name:"창 상태 적용"});
  await waitFor(()=>expect(screen.getByRole("button",{name:"이전 창 상태 새로 고침"}).hasAttribute("disabled")).toBe(false));
  fireEvent.click(screen.getByRole("button",{name:"이전 창 상태 새로 고침"}));
  await waitFor(()=>expect(screen.getByRole("checkbox").hasAttribute("disabled")).toBe(false));
  expect(screen.getByRole("button",{name:"창 상태 적용"})).toBeTruthy();
  expect(call.mock.calls.some(([,method])=>method==="cancel_window_import")).toBe(false);
});
