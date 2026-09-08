import {act,cleanup,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {afterEach,beforeEach,expect,it,vi} from "vitest";
import {describe as describeProduct,type Description} from "@devbox/product-shell/api";
import SourceCleanupScope from "./SourceCleanupScope";
import {componentCall} from "./native";
vi.mock("./native",()=>({componentCall:vi.fn()}));
const call=vi.mocked(componentCall);
let description:Description;
const empty={available:[{id:"native-sibling",root:"C:/native/sibling"}],hasApproval:false,selectedIds:[]};
const preview={previewId:"native-preview",members:[{id:"native-sibling",root:"C:/native/sibling",review:{executionKeys:["core.hooksPath"],sources:[]}}]};
beforeEach(async()=>{
  description=await describeProduct("workspace");call.mockReset();
  call.mockImplementation(async(_description,_component,method)=>method==="cleanup_scope_status"?empty:method==="preview_cleanup_scope"?preview:null);
});
afterEach(cleanup);
async function click(name:string) {
  const button=await screen.findByRole("button",{name});
  await waitFor(()=>expect((button as HTMLButtonElement).disabled).toBe(false));
  await act(async()=>{fireEvent.click(button);});
}
it("enumerates only on request, selects no sibling by default and approves only a native token",async()=>{
  const changed=vi.fn();
  render(<SourceCleanupScope description={description} enabled onBusyChange={vi.fn()} onDirtyChange={vi.fn()} onChange={changed}/>);
  expect(call).not.toHaveBeenCalled();
  await click("정리 범위 확인");
  const checkbox=screen.getByRole("checkbox",{name:"C:/native/sibling"});
  expect((checkbox as HTMLInputElement).checked).toBe(false);
  expect((screen.getByRole("button",{name:"선택한 정리 범위 검토"}) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(checkbox);await click("선택한 정리 범위 검토");
  expect(call).toHaveBeenCalledWith(description,"workspace.source","preview_cleanup_scope",{worktreeIds:["native-sibling"]},"source");
  expect(changed).not.toHaveBeenCalled();
  await click("검토한 정리 범위 승인");
  expect(call).toHaveBeenCalledWith(description,"workspace.source","approve_cleanup_scope",{previewId:"native-preview"},"source");
  expect(changed).toHaveBeenCalledOnce();
  expect(call.mock.calls.every(([, ,method])=>!method.startsWith("repo_")&&method!=="create_worktree")).toBe(true);
});
it("retains selected inputs on cancelled review and allows private revocation when Git is unavailable",async()=>{
  const props={description,enabled:true,onBusyChange:vi.fn(),onDirtyChange:vi.fn(),onChange:vi.fn()};
  const view=render(<SourceCleanupScope {...props}/>);
  await click("정리 범위 확인");fireEvent.click(screen.getByRole("checkbox"));await click("선택한 정리 범위 검토");
  await click("정리 범위 검토 취소");
  expect((screen.getByRole("checkbox") as HTMLInputElement).checked).toBe(true);
  expect(call).toHaveBeenCalledWith(description,"workspace.source","cancel_cleanup_scope",{previewId:"native-preview"},"source");
  call.mockImplementation(async(_description,_component,method)=>method==="cleanup_scope_status"?{...empty,hasApproval:true,selectedIds:["native-sibling"]}:null);
  await click("범위 선택 되돌리기");await click("정리 범위 확인");
  view.rerender(<SourceCleanupScope {...props} enabled={false}/>);
  await click("정리 범위 승인 철회");
  expect(call).toHaveBeenCalledWith(description,"workspace.source","revoke_cleanup_scope",{},"source");
});
it("retires a preview arriving after the surface closes",async()=>{
  let resolve!:(value:unknown)=>void;
  call.mockImplementation(async(_description,_component,method)=>method==="cleanup_scope_status"?empty:method==="preview_cleanup_scope"?new Promise(done=>{resolve=done;}):null);
  const view=render(<SourceCleanupScope description={description} enabled onBusyChange={vi.fn()} onDirtyChange={vi.fn()} onChange={vi.fn()}/>);
  await click("정리 범위 확인");fireEvent.click(screen.getByRole("checkbox"));await click("선택한 정리 범위 검토");
  view.unmount();await act(async()=>resolve(preview));
  expect(call).toHaveBeenCalledWith(description,"workspace.source","cancel_cleanup_scope",{previewId:"native-preview"},"source");
});
