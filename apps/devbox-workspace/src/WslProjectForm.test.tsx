import {afterEach,expect,it,vi} from "vitest";
import {act,cleanup,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {assertNoA11yViolations} from "@devbox/a11y/testing";
import {nativeCall} from "./native";
import WslProjectForm from "./WslProjectForm";
vi.mock("./native",()=>({nativeCall:vi.fn()}));
const call=vi.mocked(nativeCall);
const distro={id:"native-distro-id",name:"합성 Ubuntu",version:2,running:false};
const preview={previewId:"native-preview",binding:{root:"/home/test/한글 project",target:{kind:"wsl" as const,distroId:distro.id}},discovery:{kind:"newProject" as const}};
afterEach(()=>{cleanup();call.mockReset();});

it("lists without starting and requires an explicit stopped-distro choice before a preview",async()=>{
  call.mockImplementation(async(_component,method)=>method==="list_wsl_distros"?[distro]:preview);
  const reviewed=vi.fn();
  const {container}=render(<WslProjectForm disabled={false} onBusyChange={()=>{}} onReviewed={reviewed}/>);
  await screen.findByRole("option",{name:"합성 Ubuntu · 중지됨"});
  expect(call.mock.calls.map(([,method])=>method)).toEqual(["list_wsl_distros"]);
  fireEvent.change(screen.getByLabelText("WSL 배포판"),{target:{value:distro.id}});
  fireEvent.change(screen.getByLabelText("Linux 프로젝트 폴더"),{target:{value:preview.binding.root}});
  expect((screen.getByRole("button",{name:"WSL 폴더 확인"}) as HTMLButtonElement).disabled).toBe(true);
  await assertNoA11yViolations(container);
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(screen.getByRole("button",{name:"WSL 폴더 확인"}));
  await waitFor(()=>expect(reviewed).toHaveBeenCalledWith(preview,"한글 project"));
  expect(call).toHaveBeenCalledWith("workspace.registry","preview_wsl",{distroId:distro.id,root:preview.binding.root,startStopped:true});
  expect(call.mock.calls.some(([,method])=>/apply|select_project|trust/.test(method))).toBe(false);
});

it("cancels a late native preview after the form closes",async()=>{
  let complete!:(value:typeof preview)=>void;
  call.mockImplementation(async(_component,method)=>method==="list_wsl_distros"?[{...distro,running:true}]:method==="preview_wsl"?new Promise(resolve=>{complete=resolve;}):{});
  const reviewed=vi.fn();
  const view=render(<WslProjectForm disabled={false} onBusyChange={()=>{}} onReviewed={reviewed}/>);
  await screen.findByRole("option",{name:"합성 Ubuntu · 실행 중"});
  fireEvent.change(screen.getByLabelText("WSL 배포판"),{target:{value:distro.id}});
  fireEvent.change(screen.getByLabelText("Linux 프로젝트 폴더"),{target:{value:preview.binding.root}});
  fireEvent.click(screen.getByRole("button",{name:"WSL 폴더 확인"}));
  expect(call).toHaveBeenCalledWith("workspace.registry","preview_wsl",{distroId:distro.id,root:preview.binding.root,startStopped:false});
  view.unmount();
  await act(async()=>complete(preview));
  await waitFor(()=>expect(call).toHaveBeenCalledWith("workspace.registry","cancel_registration",{previewId:preview.previewId}));
  expect(reviewed).not.toHaveBeenCalled();
});
