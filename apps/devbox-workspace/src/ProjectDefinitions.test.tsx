import {act,cleanup,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {afterEach,beforeEach,expect,it,vi} from "vitest";
import {describe as describeProduct,type Description} from "@devbox/product-shell/api";
import ProjectDefinitions from "./ProjectDefinitions";
import {componentCall,nativeCall} from "./native";
vi.mock("./native",()=>({componentCall:vi.fn(),nativeCall:vi.fn().mockResolvedValue(null)}));
const call=vi.mocked(componentCall);
let description:Description;
const view={registryRevision:2,project:{schemaVersion:1},local:{schemaVersion:1},effective:{tasks:{dev:{source:"package.json",selector:"dev"}},toolchains:{}},sources:["package.json"],unavailableSources:[] as string[],definitionsTrusted:false,hasApproval:false};
beforeEach(async()=>{
  vi.clearAllMocks();call.mockReset();
  description={...await describeProduct("workspace"),context:{projectId:"project",worktreeId:"tree",revision:2,target:{kind:"windows"}}};
  call.mockImplementation(async(_description,_component,method)=>method==="preview_trust"?{previewId:"native-token",definition:view}:method==="load"?view:{});
});
afterEach(cleanup);

it("loads without approval and submits only an explicitly reviewed native token",async()=>{
  const dirty=vi.fn();const changed=vi.fn();
  render(<ProjectDefinitions description={description} onDirtyChange={dirty} onChanged={changed}/>);
  expect(call).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button",{name:"프로젝트 설정"}));
  fireEvent.click(await screen.findByRole("button",{name:"실행 정의 검토"}));
  await screen.findByRole("heading",{name:"이 실행 정의를 승인할까요?"});
  expect(call.mock.calls.some(([, ,method])=>method==="approve_trust")).toBe(false);
  await waitFor(()=>expect(dirty).toHaveBeenLastCalledWith(true));
  fireEvent.click(screen.getByRole("button",{name:"검토한 실행 정의 승인"}));
  await waitFor(()=>expect(call).toHaveBeenCalledWith(description,"workspace.definitions","approve_trust",{previewId:"native-token"},"overview"));
  await waitFor(()=>expect(changed).toHaveBeenCalledTimes(1));
});

it("keeps definitions readable and revocable when a source is unavailable",async()=>{
  call.mockResolvedValue({...view,sources:[],unavailableSources:["missing.json"],hasApproval:true});
  render(<ProjectDefinitions description={description} onDirtyChange={vi.fn()} onChanged={vi.fn()}/>);
  fireEvent.click(screen.getByRole("button",{name:"프로젝트 설정"}));
  expect(await screen.findByText(/확인할 수 없는 실행 소스: missing.json/)).toBeTruthy();
  expect((screen.getByRole("button",{name:"실행 정의 검토"}) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole("button",{name:"실행 정의 승인 철회"}));
  await waitFor(()=>expect(call).toHaveBeenCalledWith(description,"workspace.definitions","revoke_trust",{revision:2},"overview"));
});

it("cancels a late preview after a context change instead of presenting it for approval",async()=>{
  let finish!:(value:unknown)=>void;
  call.mockImplementation(async(_description,_component,method)=>method==="preview_trust"?new Promise(resolve=>{finish=resolve;}):view);
  const props={onDirtyChange:vi.fn(),onChanged:vi.fn()};
  const rendered=render(<ProjectDefinitions description={description} {...props}/>);
  fireEvent.click(screen.getByRole("button",{name:"프로젝트 설정"}));
  fireEvent.click(await screen.findByRole("button",{name:"실행 정의 검토"}));
  rendered.rerender(<ProjectDefinitions description={{...description,context:{...description.context!,revision:3}}} {...props}/>);
  await act(async()=>finish({previewId:"late-token",definition:view}));
  expect(nativeCall).toHaveBeenCalledWith("workspace.definitions","cancel",{previewId:"late-token"});
  expect(screen.queryByRole("heading",{name:"이 실행 정의를 승인할까요?"})).toBeNull();
  expect(call.mock.calls.some(([, ,method])=>method==="approve_trust")).toBe(false);
});
