import {afterEach,beforeEach,expect,it,vi} from "vitest";
import {cleanup,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {assertNoA11yViolations} from "@devbox/a11y/testing";
import type {Description} from "@devbox/product-shell/api";
import LegacyRecoveryImport from "./LegacyRecoveryImport";
import {componentCall} from "./native";
vi.mock("./native",()=>({componentCall:vi.fn(),issueMessage:(issue:string)=>issue}));
const call=vi.mocked(componentCall),description={} as Description;
const preview={previewId:"native-preview",candidate:{recovery:{entries:[{path:"C:/선택한 폴더/file.txt",content:"unsaved",base_hash:null,snapshot_at_ms:1}]},skippedEntries:1},currentEntries:[{path:"C:/선택한 폴더/file.txt",content:"new buffer"}],conflictingPaths:["C:/선택한 폴더/file.txt"],conflict:true,alreadyImported:false,restoring:false};
afterEach(cleanup);
beforeEach(()=>{call.mockReset();});
it("requires replacement review and submits only a native token before reloading the editor",async()=>{
  call.mockImplementation(async(_description,_component,method)=>{
    if(method==="legacy_snapshot_job")return {id:"verified-job",source:"code-pad",phase:"ready"};
    if(method==="preview_recovery_import")return preview;
    if(method==="apply_recovery_import")return {importedEntries:1,reused:false};
    throw new Error(`unexpected request: ${method}`);
  });
  const onApplied=vi.fn();
  const {container}=render(<LegacyRecoveryImport description={description} disabled={false} onApplied={onApplied} onBusyChange={vi.fn()}/>);
  expect(call).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button",{name:"복구 버퍼 가져오기 검토"}));
  const apply=await screen.findByRole("button",{name:"검토한 복구 버퍼 가져오기"});
  expect(apply.hasAttribute("disabled")).toBe(true);
  expect(screen.getByText(/가져오지 않는 버퍼 1개/)).toBeTruthy();
  await assertNoA11yViolations(container);
  fireEvent.click(screen.getByRole("checkbox"));fireEvent.click(apply);
  await screen.findByText(/미저장 버퍼 1개를 가져왔습니다/);
  expect(call).toHaveBeenCalledWith(description,"workspace.files","preview_recovery_import",{jobId:"verified-job"},"files");
  expect(call).toHaveBeenCalledWith(description,"workspace.files","apply_recovery_import",{previewId:"native-preview",replaceExisting:true},"files");
  expect(onApplied).toHaveBeenCalledTimes(1);
});
it("cancels a late native preview and never applies it after unmount",async()=>{
  let resolvePreview!:(value:unknown)=>void;
  call.mockImplementation(async(_description,_component,method)=>{
    if(method==="legacy_snapshot_job")return {id:"job",source:"code-pad",phase:"ready"};
    if(method==="preview_recovery_import")return new Promise(resolve=>{resolvePreview=resolve;});
    return null;
  });
  const view=render(<LegacyRecoveryImport description={description} disabled={false} onApplied={vi.fn()} onBusyChange={vi.fn()}/>);
  fireEvent.click(screen.getByRole("button",{name:"복구 버퍼 가져오기 검토"}));
  await waitFor(()=>expect(resolvePreview).toBeTypeOf("function"));
  view.unmount();resolvePreview(preview);
  await waitFor(()=>expect(call).toHaveBeenCalledWith(description,"workspace.files","cancel_recovery_import",{previewId:"native-preview"},"files"));
  expect(call.mock.calls.some((args)=>args[2]==="apply_recovery_import")).toBe(false);
});
it("previews native history IDs and preserves the current editor when apply fails",async()=>{
  call.mockImplementation(async(_description,_component,method)=>{
    if(method==="list_recovery_history")return {items:[{id:"native-backup",entries:1,issue:null}],unrecognized:0};
    if(method==="preview_recovery_restore")return {...preview,restoring:true,conflict:false};
    if(method==="apply_recovery_import")throw new Error("복구 버퍼이 변경되었습니다.");
    return null;
  });
  const onApplied=vi.fn();
  render(<LegacyRecoveryImport description={description} disabled={false} onApplied={onApplied} onBusyChange={vi.fn()}/>);
  fireEvent.click(screen.getByRole("button",{name:"이전 복구 버퍼 목록"}));
  fireEvent.click(await screen.findByRole("button",{name:"이전 복구 버퍼 1 복원 검토"}));
  fireEvent.click(await screen.findByRole("button",{name:"검토한 이전 복구 버퍼 복원"}));
  await screen.findByRole("alert");
  expect(onApplied).not.toHaveBeenCalled();
  expect(call).toHaveBeenCalledWith(description,"workspace.files","preview_recovery_restore",{backupId:"native-backup"},"files");
});
