import {afterEach,beforeEach,expect,it,vi} from "vitest";
import {cleanup,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {assertNoA11yViolations} from "@devbox/a11y/testing";
import type {Description} from "@devbox/product-shell/api";
import LegacyLspImport from "./LegacyLspImport";
import {componentCall} from "./native";
vi.mock("./native",()=>({componentCall:vi.fn(),issueMessage:(issue:string)=>issue}));
const call=vi.mocked(componentCall),description={context:{projectId:"project"}} as Description;
const preview={previewId:"native-preview",config:{enabled:false,workspace_root:"C:/new",server_by_language:{rust:{kind:"custom",executable:"missing-server",args:[]}}},currentConfig:{enabled:true,workspace_root:"C:/current"},conflict:true,alreadyImported:false,restoring:false};
afterEach(cleanup);
beforeEach(()=>{call.mockReset();});
it("reviews disabled configuration and sends only a one-use token after explicit replacement",async()=>{
  call.mockImplementation(async(_description,_component,method)=>{
    if(method==="legacy_snapshot_job")return {id:"verified-job",source:"code-pad",phase:"ready"};
    if(method==="preview_lsp_config_import")return preview;
    if(method==="apply_lsp_config_import")return {reused:false,restored:false};
    throw new Error(`unexpected request: ${method}`);
  });
  const onApplied=vi.fn();
  const {container}=render(<LegacyLspImport description={description} disabled={false} onApplied={onApplied} onBusyChange={vi.fn()}/>);
  expect(call).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button",{name:"LSP 설정 가져오기 검토"}));
  const apply=await screen.findByRole("button",{name:"검토한 LSP 설정 가져오기"});
  expect(apply.hasAttribute("disabled")).toBe(true);
  expect(screen.getByText(/가져올 설정 · LSP 비활성/)).toBeTruthy();
  await assertNoA11yViolations(container);
  fireEvent.click(screen.getByRole("checkbox"));fireEvent.click(apply);
  await screen.findByText(/LSP 설정을 가져왔습니다/);
  expect(call).toHaveBeenCalledWith(description,"workspace.lsp","apply_lsp_config_import",{previewId:"native-preview",replaceExisting:true},"files");
  expect(onApplied).toHaveBeenCalledTimes(1);
  expect(call.mock.calls.some(args=>/start|approve|install/.test(args[2]))).toBe(false);
});
it("cancels a late native preview after unmount",async()=>{
  let resolvePreview!:(value:unknown)=>void;
  call.mockImplementation(async(_description,_component,method)=>{
    if(method==="legacy_snapshot_job")return {id:"job",source:"code-pad",phase:"ready"};
    if(method==="preview_lsp_config_import")return new Promise(resolve=>{resolvePreview=resolve;});
    return null;
  });
  const view=render(<LegacyLspImport description={description} disabled={false} onApplied={vi.fn()} onBusyChange={vi.fn()}/>);
  fireEvent.click(screen.getByRole("button",{name:"LSP 설정 가져오기 검토"}));
  await waitFor(()=>expect(resolvePreview).toBeTypeOf("function"));
  view.unmount();resolvePreview(preview);
  await waitFor(()=>expect(call).toHaveBeenCalledWith(description,"workspace.lsp","cancel_lsp_config_import",{previewId:"native-preview"},"files"));
  expect(call.mock.calls.some(args=>args[2]==="apply_lsp_config_import")).toBe(false);
});
it("keeps current editor settings when restoring a native history item fails",async()=>{
  call.mockImplementation(async(_description,_component,method)=>{
    if(method==="list_lsp_config_history")return {items:[{id:"native-backup",languages:1,issue:null}],unrecognized:0};
    if(method==="preview_lsp_config_restore")return {...preview,restoring:true,conflict:false};
    if(method==="apply_lsp_config_import")throw new Error("설정이 변경되었습니다.");
    return null;
  });
  const onApplied=vi.fn();
  render(<LegacyLspImport description={description} disabled={false} onApplied={onApplied} onBusyChange={vi.fn()}/>);
  fireEvent.click(screen.getByRole("button",{name:"이전 LSP 설정 목록"}));
  fireEvent.click(await screen.findByRole("button",{name:"이전 LSP 설정 1 복원 검토"}));
  fireEvent.click(await screen.findByRole("button",{name:"검토한 이전 LSP 설정 복원"}));
  await screen.findByRole("alert");
  expect(onApplied).not.toHaveBeenCalled();
  expect(call).toHaveBeenCalledWith(description,"workspace.lsp","preview_lsp_config_restore",{backupId:"native-backup"},"files");
});
