import {beforeEach, expect, it, vi} from "vitest";
const {invoke, hosted} = vi.hoisted(() => ({invoke:vi.fn(), hosted:{value:false}}));
vi.mock("../transport", () => ({componentInvoke:() => invoke, isProductHosted:() => hosted.value}));
import {deleteFileAction, renameFileAction, loadRecovery, saveRecovery, saveSession} from "./api";
import type {SessionState} from "./types";
beforeEach(() => {invoke.mockReset();hosted.value=false;});

it("sends session revisions only to the product owner and returns the committed revision", async () => {
  const session:SessionState={version:1,workspace_folder:null,docs:[],views:[[],[]],active_view:0,active_doc_by_view:[null,null],recent_files:[]};
  await saveSession(session,"ignored-standalone");
  expect(invoke).toHaveBeenLastCalledWith("save_session",{session});
  hosted.value=true;
  invoke.mockResolvedValueOnce({nativeRevision:"committed"});
  expect(await saveSession(session,"reviewed")).toBe("committed");
  expect(invoke).toHaveBeenLastCalledWith("save_session",{session,nativeRevision:"reviewed"});
});

it("encodes native file action snapshots without losing timestamp precision or the native revision", async () => {
  const snapshot = {path:"C:/한글/file.txt", mtimeNanos:"9223372036854775807", size:6, contentHash:"hash", nativeRevision:"native-open"};
  invoke.mockResolvedValue(null);
  await renameFileAction(snapshot, "renamed.txt");
  expect(invoke).toHaveBeenLastCalledWith("rename_file_action", {request:{path:snapshot.path, expectedMtimeNanos:snapshot.mtimeNanos,
    expectedSize:6, expectedContentHash:"hash", nativeRevision:"native-open", newName:"renamed.txt"}});
  await deleteFileAction(snapshot);
  expect(invoke).toHaveBeenLastCalledWith("delete_file_action", {request:{path:snapshot.path, expectedMtimeNanos:snapshot.mtimeNanos,
    expectedSize:6, expectedContentHash:"hash", nativeRevision:"native-open"}});
});

it("preserves the native recovery schema while exposing camel-case UI fields", async () => {
  const wire = {path:"C:/한글/file.txt", content:"unsaved", base_hash:"base", snapshot_at_ms:1234};
  invoke.mockResolvedValueOnce([wire]);
  const entries = await loadRecovery();
  expect(entries).toEqual([{path:wire.path, content:"unsaved", baseHash:"base", snapshotAtMs:1234}]);
  await saveRecovery(entries);
  expect(invoke).toHaveBeenLastCalledWith("save_recovery", {entries:[wire]});
});
