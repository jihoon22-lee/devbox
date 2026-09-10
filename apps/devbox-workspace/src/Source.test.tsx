import {StrictMode,useLayoutEffect} from "react";
import {act,cleanup,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {afterEach,beforeEach,expect,it,vi} from "vitest";
import {describe as describeProduct,type Description} from "@devbox/product-shell/api";
import Source from "./Source";
import {componentCall} from "./native";
vi.mock("./native",()=>({componentCall:vi.fn()}));
const call=vi.mocked(componentCall);
let description:Description;
let approved=false;
const review={executable:"C:\\Git\\git.exe",sources:[{path:"C:\\fixture\\.git\\config",kind:"config",digest:"a".repeat(64)}],executionKeys:["credential.helper"],environmentKeys:["SSH_ASKPASS"]};
const status=()=>({approved,hasApproval:approved,review});
beforeEach(async()=>{
  approved=false;call.mockReset();
  description={...await describeProduct("workspace"),context:{projectId:"project",worktreeId:"tree",revision:1,target:{kind:"windows"}}};
  call.mockImplementation(async(_description,_component,method)=>{
    if(method==="approve_trust")approved=true;
    if(method==="revoke_trust")approved=false;
    return method==="preview_trust"?{previewId:"native-preview",status:status()}:status();
  });
});
afterEach(cleanup);
async function click(name:string) {
  const button=await screen.findByRole("button",{name});
  await waitFor(()=>expect((button as HTMLButtonElement).disabled).toBe(false));
  await act(async()=>{fireEvent.click(button);});
}
it("requires explicit Git review and preserves the actual commit draft across revoke and reapproval",async()=>{
  const busy=vi.fn();const dirty=vi.fn();
  render(<StrictMode><Source description={description} root="C:\\fixture" onBusyChange={busy} onDirtyChange={dirty}/></StrictMode>);
  await click("Git 실행 검토");
  expect(await screen.findByRole("heading",{name:"이 Git 실행 근거를 신뢰할까요?"})).toBeTruthy();
  expect(screen.queryByRole("textbox",{name:"커밋 메시지"})).toBeNull();
  expect(call.mock.calls.some(([, ,method])=>method==="approve_trust")).toBe(false);
  await click("Git 승인 검토 취소");
  await waitFor(()=>expect(busy).toHaveBeenLastCalledWith(false));
  await click("Git 실행 검토");await click("검토한 Git 실행 승인");
  const message=await screen.findByRole("textbox",{name:"커밋 메시지"});
  await waitFor(()=>expect((message.closest(".workspace-source-actions") as HTMLFieldSetElement).disabled).toBe(false));
  await act(async()=>{fireEvent.change(message,{target:{value:"keep this commit draft"}});});
  await waitFor(()=>expect(dirty).toHaveBeenLastCalledWith(true));
  await click("Git 실행 승인 철회");
  await screen.findByText("Git 실행 승인을 철회했습니다.");
  expect((message as HTMLTextAreaElement).value).toBe("keep this commit draft");
  expect((message.closest(".workspace-source-actions") as HTMLFieldSetElement).disabled).toBe(true);
  await click("Git 실행 검토");await click("검토한 Git 실행 승인");
  await waitFor(()=>expect((message.closest(".workspace-source-actions") as HTMLFieldSetElement).disabled).toBe(false));
  expect((message as HTMLTextAreaElement).value).toBe("keep this commit draft");
  expect(call.mock.calls.filter(([, ,method])=>method==="approve_trust").every(([,component,,args,route])=>component==="workspace.source"&&route==="source"&&(args as {previewId:string}).previewId==="native-preview")).toBe(true);
});
it("discards a late native review after unmount without approving it",async()=>{
  let finish!:(value:unknown)=>void;
  call.mockImplementation(async(_description,_component,method)=>method==="preview_trust"?new Promise(resolve=>{finish=resolve;}):status());
  const rendered=render(<Source description={description} root="C:\\fixture" onBusyChange={vi.fn()} onDirtyChange={vi.fn()}/>);
  await click("Git 실행 검토");
  rendered.unmount();
  await act(async()=>finish({previewId:"late-preview",status:status()}));
  expect(call).toHaveBeenCalledWith(description,"workspace.source","cancel_trust",{previewId:"late-preview"},"source");
  expect(call.mock.calls.some(([, ,method])=>method==="approve_trust")).toBe(false);
});
it("can revoke stored permission when external Git configuration cannot be inspected",async()=>{
  call.mockImplementation(async(_description,_component,method)=>{
    if(method==="trust_status")throw new Error("Git 설정 파일을 확인할 수 없습니다.");
    return {approved:false};
  });
  render(<Source description={description} root="C:\\fixture" onBusyChange={vi.fn()} onDirtyChange={vi.fn()}/>);
  await screen.findByRole("alert");
  await click("Git 실행 승인 철회");
  await screen.findByText("Git 실행 승인을 철회했습니다.");
  expect(call.mock.calls.filter(([, ,method])=>method==="trust_status")).toHaveLength(1);
});

it("blocks review in the first committed frame while initial inspection is pending",async()=>{
  let finish!:(value:unknown)=>void;
  call.mockImplementation(async(_description,_component,method)=>method==="trust_status"?new Promise(resolve=>{finish=resolve;}):{previewId:"native-preview",status:status()});
  let firstFrameDisabled=false;
  function FirstFrame() {
    useLayoutEffect(()=>{firstFrameDisabled=(screen.getByRole("button",{name:"Git 실행 검토"}) as HTMLButtonElement).disabled;},[]);
    return <Source description={description} root="C:\\fixture" onBusyChange={vi.fn()} onDirtyChange={vi.fn()}/>;
  }
  render(<FirstFrame/>);
  expect(firstFrameDisabled).toBe(true);
  expect(screen.getByRole("button",{name:"Git 실행 검토"})).toHaveProperty("disabled",true);
  await act(async()=>finish(status()));
  await click("Git 실행 검토");
  expect(await screen.findByRole("heading",{name:"이 Git 실행 근거를 신뢰할까요?"})).toBeTruthy();
  expect(call.mock.calls.filter(([, ,method])=>method==="preview_trust")).toHaveLength(1);
});
