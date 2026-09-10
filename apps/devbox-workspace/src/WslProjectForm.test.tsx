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


it("reviews WSL template defaults against the selected distro without registering or granting trust",async()=>{
  const template={id:"native-template",local:true,template:{id:"template",name:"WSL 웹 기본값",windowsPath:null,wsl:{distro:"Missing preset",path:"/missing/preset"},gitRoot:null,expectedPorts:[4321],runManagerServiceIds:["saved-service"]}};
  call.mockImplementation(async(_component,method)=>method==="list_wsl_distros"?[{...distro,running:true}]:preview);
  const reviewed=vi.fn();
  const {container}=render(<WslProjectForm templates={[template]} disabled={false} onBusyChange={()=>{}} onReviewed={reviewed}/>);
  await screen.findByRole("option",{name:"합성 Ubuntu · 실행 중"});
  fireEvent.change(screen.getByLabelText("WSL 프로젝트 템플릿"),{target:{value:template.id}});
  expect(screen.getByText("4321")).toBeTruthy();
  expect(call.mock.calls.map(([,method])=>method)).toEqual(["list_wsl_distros"]);
  fireEvent.change(screen.getByLabelText("WSL 배포판"),{target:{value:distro.id}});
  fireEvent.change(screen.getByLabelText("Linux 프로젝트 폴더"),{target:{value:preview.binding.root}});
  await assertNoA11yViolations(container);
  fireEvent.click(screen.getByRole("button",{name:"WSL 폴더 확인"}));
  await waitFor(()=>expect(reviewed).toHaveBeenCalledWith(preview,template.template.name));
  expect(call).toHaveBeenCalledWith("workspace.registry","preview_template_profile_wsl",{templateId:template.id,distroId:distro.id,root:preview.binding.root,name:template.template.name,startStopped:false});
  expect(call.mock.calls.some(([,method])=>/apply|select_project|trust/.test(method))).toBe(false);
});

it("keeps an imported WSL profile unavailable until its saved distro exists and explicit start is selected",async()=>{
  const profile={id:"imported-profile",local:true,profile:{id:"original-profile",name:"보관한 WSL 프로필",windowsPath:null,wsl:{distro:distro.name.toLowerCase(),path:preview.binding.root},gitRoot:null,expectedPorts:[4321],runManagerServiceIds:[],environment:null}};
  let present=false;
  call.mockImplementation(async(_component,method)=>method==="list_wsl_distros"?(present?[distro]:[{...distro,name:"Other distribution"}]):preview);
  const reviewed=vi.fn();
  render(<WslProjectForm profile={profile} disabled={false} onBusyChange={()=>{}} onReviewed={reviewed}/>);
  await screen.findByText(/프로필의 배포판.*찾을 수 없습니다/);
  expect((screen.getByRole("button",{name:"WSL 폴더 확인"}) as HTMLButtonElement).disabled).toBe(true);
  expect((screen.getByLabelText("Linux 프로젝트 폴더") as HTMLInputElement).readOnly).toBe(true);
  expect(call.mock.calls.map(([,method])=>method)).toEqual(["list_wsl_distros"]);
  present=true;
  fireEvent.click(screen.getByRole("button",{name:"WSL 목록 새로 고침"}));
  await screen.findByRole("checkbox");
  expect((screen.getByRole("button",{name:"WSL 폴더 확인"}) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(screen.getByRole("button",{name:"WSL 폴더 확인"}));
  await waitFor(()=>expect(reviewed).toHaveBeenCalledWith(preview,profile.profile.name));
  expect(call).toHaveBeenCalledWith("workspace.registry","preview_imported_profile_wsl",{importedId:profile.id,distroId:distro.id,startStopped:true});
});
