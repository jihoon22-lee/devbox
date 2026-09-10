import {afterEach,beforeEach,expect,it,vi} from "vitest";
import {cleanup,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {assertNoA11yViolations} from "@devbox/a11y/testing";
import type {Description} from "@devbox/product-shell/api";
import LegacySessionImport from "./LegacySessionImport";
import {componentCall} from "./native";
vi.mock("./native",()=>({componentCall:vi.fn(),issueMessage:(issue:string)=>issue}));
const call=vi.mocked(componentCall),description={} as Description;
const preview={previewId:"native-preview",candidate:{session:{docs:[{id:"old-id",path:"C:\\선택한 폴더\\file.txt",cursor:3,bookmarks:[1,4]}],recent_files:[]},skippedDocuments:1,skippedRecentFiles:2},currentDocuments:2,currentRecentFiles:3,conflict:true,alreadyImported:false,restoring:false};
afterEach(cleanup);
beforeEach(()=>{call.mockReset();});
it.each(["code-pad","code-pad-legacy"])("requires replacement review and submits only a native token before reloading the editor (%s)",async(source)=>{
  call.mockImplementation(async(_description,_component,method)=>{
    if(method==="legacy_snapshot_job")return {id:"verified-job",source,phase:"ready"};
    if(method==="preview_session_import")return preview;
    if(method==="apply_session_import")return {importedDocuments:1,importedRecentFiles:0,reused:false};
    throw new Error(`unexpected request: ${method}`);
  });
  const onApplied=vi.fn();
  const {container}=render(<LegacySessionImport description={description} disabled={false} onApplied={onApplied} onBusyChange={vi.fn()}/>);
  expect(call).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button",{name:"세션 가져오기 검토"}));
  const apply=await screen.findByRole("button",{name:"검토한 세션 가져오기"});
  expect(apply.hasAttribute("disabled")).toBe(true);
  expect(screen.getByText(/복원하지 않는 파일 1개/)).toBeTruthy();
  await assertNoA11yViolations(container);
  fireEvent.click(screen.getByRole("checkbox"));fireEvent.click(apply);
  await screen.findByText(/파일 1개 · 최근 파일 0개를 가져왔습니다/);
  expect(call).toHaveBeenCalledWith(description,"workspace.files","preview_session_import",{jobId:"verified-job"},"files");
  expect(call).toHaveBeenCalledWith(description,"workspace.files","apply_session_import",{previewId:"native-preview",replaceExisting:true},"files");
  expect(onApplied).toHaveBeenCalledTimes(1);
});
it("cancels a late native preview and never applies it after unmount",async()=>{
  let resolvePreview!:(value:unknown)=>void;
  call.mockImplementation(async(_description,_component,method)=>{
    if(method==="legacy_snapshot_job")return {id:"job",source:"code-pad",phase:"ready"};
    if(method==="preview_session_import")return new Promise(resolve=>{resolvePreview=resolve;});
    return null;
  });
  const view=render(<LegacySessionImport description={description} disabled={false} onApplied={vi.fn()} onBusyChange={vi.fn()}/>);
  fireEvent.click(screen.getByRole("button",{name:"세션 가져오기 검토"}));
  await waitFor(()=>expect(resolvePreview).toBeTypeOf("function"));
  view.unmount();resolvePreview(preview);
  await waitFor(()=>expect(call).toHaveBeenCalledWith(description,"workspace.files","cancel_session_import",{previewId:"native-preview"},"files"));
  expect(call.mock.calls.some((args)=>args[2]==="apply_session_import")).toBe(false);
});
it("previews native history IDs and preserves the current editor when apply fails",async()=>{
  call.mockImplementation(async(_description,_component,method)=>{
    if(method==="list_session_history")return {items:[{id:"native-backup",documents:1,recentFiles:0,issue:null}],unrecognized:0};
    if(method==="preview_session_restore")return {...preview,restoring:true,conflict:false};
    if(method==="apply_session_import")throw new Error("세션이 변경되었습니다.");
    return null;
  });
  const onApplied=vi.fn();
  render(<LegacySessionImport description={description} disabled={false} onApplied={onApplied} onBusyChange={vi.fn()}/>);
  fireEvent.click(screen.getByRole("button",{name:"이전 세션 목록"}));
  fireEvent.click(await screen.findByRole("button",{name:"이전 세션 1 복원 검토"}));
  fireEvent.click(await screen.findByRole("button",{name:"검토한 이전 세션 복원"}));
  await screen.findByRole("alert");
  expect(onApplied).not.toHaveBeenCalled();
  expect(call).toHaveBeenCalledWith(description,"workspace.files","preview_session_restore",{backupId:"native-backup"},"files");
});
