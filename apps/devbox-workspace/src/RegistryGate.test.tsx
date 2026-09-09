import {afterEach, beforeEach, expect, it, vi} from "vitest";
import {cleanup, fireEvent, render, screen, waitFor} from "@testing-library/react";
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
  call.mockImplementation(async (_component, method) => method === "status" ? {phase:"selected"} : method === "snapshot" ? registry : method === "legacy_snapshot_job" ? null : method === "list_legacy_snapshots" ? {snapshots:[],unrecognized:0} : {});
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
