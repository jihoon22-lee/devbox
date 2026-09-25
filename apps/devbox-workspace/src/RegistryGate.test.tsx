import {afterEach, beforeEach, expect, it, vi} from "vitest";
import {cleanup, fireEvent, render, screen, waitFor, within} from "@testing-library/react";
import {assertNoA11yViolations} from "@devbox/a11y/testing";
import RegistryGate from "./RegistryGate";
import {nativeCall} from "./native";
vi.mock("./native", () => ({nativeCall:vi.fn(), issueMessage:()=>"저장된 설정을 읽을 수 없습니다."}));
const call = vi.mocked(nativeCall);
const emptyRegistry = {revision:1,projects:[],worktrees:[]};
const preview = {previewId:"fixture-preview",binding:{root:"C:\\fixture\\프로젝트",target:{kind:"windows"}},discovery:{kind:"newProject"}};
afterEach(cleanup);
beforeEach(() => {
  call.mockReset();
  call.mockImplementation(async (_component, method) => {
    if (method === "status") return {phase:"selected"};
    if (method === "snapshot") return emptyRegistry;
    if (method === "preview_windows") return preview;
    return {};
  });
});
it("previews and cancels registration without granting trust or writing the registry", async () => {
  const {container} = render(<RegistryGate/>);
  fireEvent.change(await screen.findByLabelText("Windows 프로젝트 폴더"), {target:{value:preview.binding.root}});
  fireEvent.click(screen.getByRole("button", {name:"폴더 확인"}));
  await screen.findByRole("heading", {name:"등록 확인"});
  expect(call.mock.calls.some(([,method])=>method === "apply_registration")).toBe(false);
  expect(screen.getByText("명령 실행에 대한 신뢰는 별도로 확인합니다.")).toBeTruthy();
  await assertNoA11yViolations(container);
  fireEvent.click(screen.getByRole("button", {name:"취소"}));
  await waitFor(()=>expect(screen.queryByRole("heading", {name:"등록 확인"})).toBeNull());
  expect(call).toHaveBeenCalledWith("workspace.registry", "cancel_registration", {previewId:"fixture-preview"});
  expect(call.mock.calls.some(([,method])=>method === "apply_registration")).toBe(false);
});
it("sends only the reviewed preview token on explicit registration", async () => {
  render(<RegistryGate/>);
  fireEvent.change(await screen.findByLabelText("Windows 프로젝트 폴더"), {target:{value:preview.binding.root}});
  fireEvent.click(screen.getByRole("button", {name:"폴더 확인"}));
  fireEvent.change(await screen.findByLabelText("프로젝트 이름"), {target:{value:"합성 프로젝트"}});
  fireEvent.click(screen.getByRole("button", {name:/^등록$/}));
  await waitFor(()=>expect(call).toHaveBeenCalledWith("workspace.registry", "apply_registration", {previewId:"fixture-preview",name:"합성 프로젝트",action:"register"}));
  expect(call.mock.calls.some(([,method])=>/trust|execute|start_workspace/.test(method))).toBe(false);
});
it("selects only an explicit native context and refreshes after successful admission", async () => {
  const context = {projectId:"project-a",worktreeId:"tree-a",revision:2,target:{kind:"windows" as const}};
  const registry = {revision:2,projects:[{id:"project-a",name:"fixture"}],worktrees:[{id:"tree-a",projectId:"project-a",revision:2,binding:preview.binding,trustedDigest:null}]};
  const refreshed = vi.fn(async () => {});
  call.mockImplementation(async (_component, method) => method === "status" ? {phase:"selected"} : method === "snapshot" ? registry : method === "legacy_snapshot_job" ? null : method === "list_window_history" ? {items:[],unrecognized:0} : method === "list_legacy_snapshots" ? {snapshots:[],unrecognized:0} : {});
  const view = render(<RegistryGate onContextChanged={refreshed}/>);
  fireEvent.click(await screen.findByRole("button", {name:"프로젝트 선택"}));
  await waitFor(() => expect(refreshed).toHaveBeenCalledTimes(1));
  expect(call).toHaveBeenCalledWith("workspace.registry", "select_project", {context});
  expect(call.mock.calls.some(([,method]) => /trust|language_server/.test(method))).toBe(false);
  view.rerender(<RegistryGate context={context} onContextChanged={refreshed}/>);
  fireEvent.click(screen.getByRole("button", {name:"프로젝트 선택 해제"}));
  await waitFor(() => expect(refreshed).toHaveBeenCalledTimes(2));
  expect(call).toHaveBeenCalledWith("workspace.registry", "clear_project", {});
});

it("reviews an explicit Source suggestion without automatically registering or selecting it",async()=>{
  const suggestedRoot={id:"created-worktree",path:"C:/created/worktree",name:"현재 프로젝트"};
  const view=render(<RegistryGate suggestedRoot={suggestedRoot}/>);
  await screen.findByRole("heading",{name:"등록 확인"});
  expect(call).toHaveBeenCalledWith("workspace.registry","preview_windows",{root:suggestedRoot.path});
  view.rerender(<RegistryGate suggestedRoot={suggestedRoot}/>);
  expect(call.mock.calls.filter(([,method])=>method==="preview_windows")).toHaveLength(1);
  expect(call.mock.calls.some(([,method])=>/apply_registration|select_project|trust/.test(method))).toBe(false);
  expect((screen.getByLabelText("프로젝트 이름") as HTMLInputElement).value).toBe(suggestedRoot.name);
});

it("reviews a concrete template profile and registers only after explicit approval",async()=>{
  const template={id:"legacy-template",name:"웹 기본값",windowsPath:null,wsl:null,gitRoot:null,expectedPorts:[4321],runManagerServiceIds:["saved-service"]};
  const imported={id:"native-template",sourceSnapshotId:"snapshot",template};
  const candidate={id:"new-native-profile",sourceSnapshotId:"snapshot",sourceTemplateId:imported.id,profile:{...template,id:"new-profile-id",windowsPath:preview.binding.root,environment:null}};
  call.mockImplementation(async(_component,method)=>{
    if(method==="status")return {phase:"selected"};
    if(method==="snapshot")return {...emptyRegistry,importedTemplates:[imported]};
    if(method==="legacy_snapshot_job")return null;
    if(method==="list_window_history")return {items:[],unrecognized:0};
    if(method==="list_legacy_snapshots")return {snapshots:[],unrecognized:0};
    if(method==="preview_template_profile_windows")return {...preview,templateProfile:candidate};
    return {};
  });
  const {container}=render(<RegistryGate/>);
  fireEvent.change(await screen.findByRole("combobox",{name:"프로젝트 템플릿"}),{target:{value:imported.id}});
  expect(within(screen.getByRole("combobox",{name:"프로젝트 템플릿"}).closest("form")!).getByText("4321")).toBeTruthy();
  expect(call.mock.calls.some(([,method])=>/preview_|apply_|select_project|trust/.test(method))).toBe(false);
  fireEvent.change(screen.getByLabelText("Windows 프로젝트 폴더"),{target:{value:preview.binding.root}});
  fireEvent.click(screen.getByRole("button",{name:"폴더 확인"}));
  await screen.findByText("선택한 템플릿으로 아래 프로필을 만들고 이 폴더에 연결합니다.");
  expect(call).toHaveBeenCalledWith("workspace.registry","preview_template_profile_windows",{templateId:imported.id,root:preview.binding.root,name:template.name});
  expect(call.mock.calls.some(([,method])=>/apply_|select_project|trust/.test(method))).toBe(false);
  await assertNoA11yViolations(container);
  fireEvent.click(screen.getByRole("button",{name:/^등록$/}));
  await waitFor(()=>expect(call).toHaveBeenCalledWith("workspace.registry","apply_registration",{previewId:preview.previewId,name:template.name,action:"register"}));
});


it("loads WSL discovery only after opening its folder form",async()=>{
  const original=call.getMockImplementation()!;
  call.mockImplementation(async(component,method,...args)=>method==="list_wsl_distros"?[]:original(component,method,...args));
  render(<RegistryGate/>);
  const open=await screen.findByRole("button",{name:"WSL 프로젝트 추가"});
  expect(call.mock.calls.some(([,method])=>method==="list_wsl_distros")).toBe(false);
  fireEvent.click(open);
  await screen.findByText("등록된 WSL 배포판이 없습니다.");
  expect(call.mock.calls.filter(([,method])=>method==="list_wsl_distros")).toHaveLength(1);
  expect(call.mock.calls.some(([,method])=>method==="preview_wsl")).toBe(false);
});

it("reviews a Source worktree proposal in its WSL distro without starting it", async () => {
  const target={kind:"wsl" as const,distroId:"native-distro-id"};
  const suggestedRoot={id:"created-wsl-worktree",path:"/home/fixture/새 worktree",name:"WSL 프로젝트",target};
  call.mockImplementation(async(_component,method)=>{
    if(method==="status")return {phase:"selected"};
    if(method==="snapshot")return emptyRegistry;
    if(method==="preview_wsl")return {...preview,binding:{root:suggestedRoot.path,target}};
    if(method==="legacy_snapshot_job")return null;
    if(method==="list_window_history")return {items:[],unrecognized:0};
    return {snapshots:[],unrecognized:0};
  });
  render(<RegistryGate suggestedRoot={suggestedRoot}/>);
  await waitFor(()=>expect(call).toHaveBeenCalledWith("workspace.registry","preview_wsl",{distroId:target.distroId,root:suggestedRoot.path,startStopped:false}));
  expect(call.mock.calls.some(([,method])=>method==="preview_windows"||method==="apply_registration")).toBe(false);
  expect(await screen.findByDisplayValue(suggestedRoot.name)).toBeTruthy();
});

it("publishes startup readiness only after its registry snapshot resolves",async()=>{
  let finish!:(value:unknown)=>void;
  const pending=new Promise(resolve=>{finish=resolve;});
  call.mockImplementation(async(_component,method)=>method==="status"?{phase:"selected"}:method==="snapshot"?pending:method==="legacy_snapshot_job"?null:{snapshots:[],unrecognized:0,items:[]});
  const ready=vi.fn();render(<RegistryGate onReady={ready}/>);
  await waitFor(()=>expect(call.mock.calls.some(([,method])=>method==="snapshot")).toBe(true));
  expect(ready).not.toHaveBeenCalled();
  expect(call.mock.calls.some(([,method])=>method==="list_legacy_snapshots")).toBe(false);
  finish(emptyRegistry);
  await waitFor(()=>expect(ready).toHaveBeenCalledOnce());
});

it("starts an empty registry automatically once",async()=>{
 let started=false;
 call.mockImplementation(async(_component,method)=>{
  if(method==="status")return {phase:started?"selected":"setup"};
  if(method==="start_empty"){started=true;return {};}
  if(method==="snapshot")return emptyRegistry;
  throw new Error(method);
 });
 render(<RegistryGate/>);
 await screen.findByLabelText("Windows 프로젝트 폴더");
 expect(call.mock.calls.filter(([,method])=>method==="start_empty")).toHaveLength(1);
 expect(screen.queryByRole("button",{name:"빈 Workspace 시작"})).toBeNull();
});
it("does not retry a failed automatic start until requested",async()=>{
 call.mockImplementation(async(_component,method)=>{if(method==="status")return {phase:"setup"};throw new Error("저장소 준비 실패");});
 render(<RegistryGate/>);
 await screen.findByText("저장소 준비 실패");
 await waitFor(()=>expect(call.mock.calls.filter(([,method])=>method==="start_empty")).toHaveLength(1));
 fireEvent.click(screen.getByRole("button",{name:"다시 시도"}));
 await waitFor(()=>expect(call.mock.calls.filter(([,method])=>method==="start_empty")).toHaveLength(2));
});
it("prepares only the store while Suite activation is pending",async()=>{
 render(<RegistryGate setupOnly/>);
 await waitFor(()=>expect(call).toHaveBeenCalledWith("workspace.registry","snapshot",{}));
 expect(screen.queryByLabelText("Windows 프로젝트 폴더")).toBeNull();
 expect(call.mock.calls.some(([,method])=>method==="preview_windows")).toBe(false);
});
