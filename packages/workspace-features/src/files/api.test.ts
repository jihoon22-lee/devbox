import {beforeEach, expect, it, vi} from "vitest";
const {invoke} = vi.hoisted(() => ({invoke:vi.fn()}));
vi.mock("../transport", () => ({componentInvoke:() => invoke, isProductHosted:() => false}));
import {deleteFileAction, renameFileAction, loadRecovery, saveRecovery} from "./api";
beforeEach(() => invoke.mockReset());

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
