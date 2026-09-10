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
    if (method === "legacy_snapshot_job") return null;
    if (method === "list_window_history") return {items:[],unrecognized:0};
    if (method === "list_legacy_snapshots") return {snapshots:[],unrecognized:0};
    if (method === "status") return {phase:"selected"};
    if (method === "snapshot") return emptyRegistry;
    if (method === "preview_windows") return preview;
    return {};
  });
});
it("requires explicit empty startup and preserves a failed store", async () => {
  call.mockImplementation(async (_component,method)=>method==="status"?{phase:"setup"}:method==="legacy_snapshot_job"?null:{snapshots:[],unrecognized:0});
  const view = render(<RegistryGate/>);
  await screen.findByRole("button", {name:"빈 Workspace 시작"});
  expect(call.mock.calls.some(([,method])=>method === "start_empty")).toBe(false);
  view.unmount();
  call.mockResolvedValue({phase:"failed",issue:"invalid_files_store"});
  render(<RegistryGate/>);
  await screen.findByRole("alert");
  expect(screen.queryByRole("button", {name:"빈 Workspace 시작"})).toBeNull();
  expect(call.mock.calls.some(([,method])=>method === "start_empty")).toBe(false);
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
it("proposes an imported folder using only its stored native record ID",async()=>{
  const profile={id:"old-profile",name:"가져온 프로젝트",windowsPath:"C:\\fixture",wsl:null,gitRoot:null,expectedPorts:[3000],runManagerServiceIds:[],environment:null};
  call.mockImplementation(async(_component,method)=>{
    if(method==="status")return {phase:"selected"};
    if(method==="snapshot")return {...emptyRegistry,importedProfiles:[{id:"native-imported",sourceSnapshotId:"snapshot",profile}]};
    if(method==="legacy_snapshot_job")return null;
    if(method==="list_window_history")return {items:[],unrecognized:0};
    if(method==="list_legacy_snapshots")return {snapshots:[],unrecognized:0};
    if(method==="preview_imported_profile_windows")return {...preview,importedProfileId:"native-imported"};
    return {};
  });
  render(<RegistryGate/>);
  fireEvent.click(await screen.findByRole("button",{name:"Windows 폴더 연결 검토"}));
  await screen.findByRole("heading",{name:"등록 확인"});
  expect(call).toHaveBeenCalledWith("workspace.registry","preview_imported_profile_windows",{importedId:"native-imported"});
  expect(call.mock.calls.some(([,method])=>/apply_registration|select_project|trust/.test(method))).toBe(false);
  expect(screen.getByText(/가져온 프로필을 이 폴더에 연결합니다/)).toBeTruthy();
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

it("reviews the saved Code Pad workspace through native job identity before registration",async()=>{
  call.mockImplementation(async(_component,method)=>{
    if(method==="status")return {phase:"selected"};
    if(method==="snapshot")return emptyRegistry;
    if(method==="list_window_history")return {items:[],unrecognized:0};
    if(method==="list_legacy_snapshots")return {snapshots:[],unrecognized:0};
    if(method==="legacy_snapshot_job")return {id:"verified-job",source:"code-pad",phase:"ready",operation:"verify",manifest:{files:[{name:"session.json",issue:null,records:1}],missing:[]}};
    if(method==="legacy_workspace")return {path:preview.binding.root,target:"windows"};
    if(method==="preview_legacy_workspace_windows")return preview;
    return {};
  });
  render(<RegistryGate/>);
  fireEvent.click(await screen.findByRole("button",{name:"마지막 작업 폴더 등록 검토"}));
  await screen.findByRole("heading",{name:"등록 확인"});
  expect(call).toHaveBeenCalledWith("workspace.registry","preview_legacy_workspace_windows",{jobId:"verified-job"});
  expect(call.mock.calls.some(([,method])=>/apply_registration|select_project|trust/.test(method))).toBe(false);
  fireEvent.click(screen.getByRole("button",{name:"취소"}));
  await waitFor(()=>expect(call).toHaveBeenCalledWith("workspace.registry","cancel_registration",{previewId:preview.previewId}));
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


it("opens the imported WSL profile form and cancels its native binding review without registration",async()=>{
  const distro={id:"selected-distro",name:"Profile Ubuntu",version:2,running:true};
  const imported={id:"native-profile",local:true,profile:{id:"old-profile",name:"보관한 Linux 프로젝트",windowsPath:null,wsl:{distro:distro.name,path:"/home/fixture/보관한 프로젝트"},gitRoot:null,expectedPorts:[4321],runManagerServiceIds:[],environment:null}};
  const wslPreview={...preview,binding:{root:imported.profile.wsl.path,target:{kind:"wsl",distroId:distro.id}},importedProfileId:imported.id};
  const original=call.getMockImplementation()!;
  call.mockImplementation(async(component,method,...args)=>method==="snapshot"?{...emptyRegistry,importedProfiles:[imported]}:method==="list_wsl_distros"?[distro]:method==="preview_imported_profile_wsl"?wslPreview:original(component,method,...args));
  render(<RegistryGate/>);
  fireEvent.click(await screen.findByRole("button",{name:"WSL 폴더 연결 검토"}));
  await screen.findByRole("option",{name:"Profile Ubuntu · 실행 중"});
  fireEvent.click(screen.getByRole("button",{name:"WSL 폴더 확인"}));
  await screen.findByRole("heading",{name:"등록 확인"});
  expect(call).toHaveBeenCalledWith("workspace.registry","preview_imported_profile_wsl",{importedId:imported.id,distroId:distro.id,startStopped:false});
  expect((screen.getByLabelText("프로젝트 이름") as HTMLInputElement).value).toBe(imported.profile.name);
  expect(call.mock.calls.some(([,method])=>/apply_registration|select_project|trust/.test(method))).toBe(false);
  fireEvent.click(screen.getByRole("button",{name:"취소"}));
  await waitFor(()=>expect(call).toHaveBeenCalledWith("workspace.registry","cancel_registration",{previewId:preview.previewId}));
});
