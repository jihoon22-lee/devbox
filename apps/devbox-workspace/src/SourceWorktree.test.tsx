import {act,cleanup,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {afterEach,beforeEach,expect,it,vi} from "vitest";
import {describe as describeProduct,type Description} from "@devbox/product-shell/api";
import SourceWorktree from "./SourceWorktree";
import {componentCall} from "./native";
vi.mock("./native",()=>({componentCall:vi.fn()}));
const call=vi.mocked(componentCall);
let description:Description;
const preview={previewId:"native-review",branch:"native-branch",targetDir:"C:/reviewed/new-tree",root:"C:/reviewed/source"};
beforeEach(async()=>{
  description=await describeProduct("workspace");call.mockReset();
  call.mockImplementation(async(_description,_component,method)=>method==="preview_worktree"?preview:method==="create_worktree"?{path:preview.targetDir}:null);
});
afterEach(cleanup);
async function click(name:string) {const button=await screen.findByRole("button",{name});await waitFor(()=>expect((button as HTMLButtonElement).disabled).toBe(false));await act(async()=>{fireEvent.click(button);});}
async function prepare() {
  fireEvent.change(screen.getByLabelText("새 branch 이름"),{target:{value:"typed-branch"}});
  fireEvent.change(screen.getByLabelText("생성할 폴더 경로"),{target:{value:"C:/typed/new-tree"}});
  await click("작업 폴더 생성 검토");
}
it("creates only a reviewed native token and proposes registration on a separate click",async()=>{
  const propose=vi.fn();
  render(<SourceWorktree description={description} enabled onBusyChange={vi.fn()} onDirtyChange={vi.fn()} onPropose={propose}/>);
  await prepare();
  expect(screen.getByText(`새 폴더: ${preview.targetDir}`)).toBeTruthy();
  expect(call.mock.calls.some(([, ,method])=>method==="create_worktree")).toBe(false);
  await click("확인한 작업 폴더 생성");
  const creation=call.mock.calls.find(([, ,method])=>method==="create_worktree")!;
  expect(creation[3]).toEqual({previewId:"native-review",operationId:expect.any(String)});
  expect(propose).not.toHaveBeenCalled();
  await click("생성한 폴더 등록 검토");
  expect(propose).toHaveBeenCalledWith(preview.targetDir);
  expect(call.mock.calls.every(([,component,, ,route])=>component==="workspace.source"&&route==="source")).toBe(true);
});
it("cancels an active creation with its operation ID and retains the failed input",async()=>{
  let reject!:(error:Error)=>void;
  call.mockImplementation(async(_description,_component,method)=>method==="preview_worktree"?preview:method==="create_worktree"?new Promise((_resolve,fail)=>{reject=fail;}):true);
  const propose=vi.fn();
  render(<SourceWorktree description={description} enabled onBusyChange={vi.fn()} onDirtyChange={vi.fn()} onPropose={propose}/>);
  await prepare();await click("확인한 작업 폴더 생성");
  const operationId=(call.mock.calls.find(([, ,method])=>method==="create_worktree")![3] as {operationId:string}).operationId;
  await click("생성 취소 요청");
  expect(call).toHaveBeenCalledWith(description,"workspace.source","repo_local_cancel",{request:{operationId}},"source");
  await act(async()=>reject(new Error("취소됨")));
  await screen.findByRole("alert");
  expect((screen.getByLabelText("새 branch 이름") as HTMLInputElement).value).toBe("typed-branch");
  expect(propose).not.toHaveBeenCalled();
});
