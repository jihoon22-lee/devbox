import {act, cleanup, fireEvent, render, waitFor} from "@testing-library/react";
import {afterEach, beforeEach, expect, it, vi} from "vitest";
const api = vi.hoisted(() => ({loadRecovery:vi.fn(), prepareRecovery:vi.fn(), applyRecoveryPreview:vi.fn(), cancelRecoveryPreview:vi.fn(), discardRecovery:vi.fn(), applyRecovery:vi.fn(), openFile:vi.fn()}));
vi.mock("../api", () => api);
vi.mock("../../transport", () => ({isProductHosted:() => true}));
import RecoveryDialog from "./RecoveryDialog";
beforeEach(() => {
  vi.resetAllMocks();
  api.loadRecovery.mockResolvedValue([{path:"C:/fixture.txt", content:"unsaved", baseHash:"hash", snapshotAtMs:1}]);
  api.prepareRecovery.mockResolvedValue({previewId:"native-preview", path:"C:/fixture.txt", before:"disk", after:"unsaved"});
  api.cancelRecoveryPreview.mockResolvedValue(undefined);
  api.discardRecovery.mockResolvedValue(undefined);
});
afterEach(cleanup);

it("cancels the native preview while keeping the recovery entry", async () => {
  const done = vi.fn();
  const view = render(<RecoveryDialog onDone={done}/>);
  fireEvent.click(await view.findByRole("button", {name:"취소"}));
  expect(done).toHaveBeenCalledWith([]);
  view.unmount();
  await waitFor(() => expect(api.cancelRecoveryPreview).toHaveBeenCalledWith("native-preview"));
  expect(api.discardRecovery).not.toHaveBeenCalled();
  expect(api.applyRecovery).not.toHaveBeenCalled();
});

it("applies only the native token and disables duplicate approval until it completes", async () => {
  let finish!: () => void;
  api.applyRecoveryPreview.mockImplementation(() => new Promise<void>(resolve => {finish=resolve;}));
  const done = vi.fn();
  const view = render(<RecoveryDialog onDone={done}/>);
  const approve = await view.findByRole("button", {name:"복구 (1)"});
  fireEvent.click(approve);
  expect((approve as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(approve);
  expect(api.applyRecoveryPreview).toHaveBeenCalledExactlyOnceWith("native-preview");
  expect(api.discardRecovery).not.toHaveBeenCalled();
  await act(async () => finish());
  expect(api.discardRecovery).toHaveBeenCalledWith("C:/fixture.txt");
  expect(done).toHaveBeenCalledWith(["C:/fixture.txt"]);
  expect(api.applyRecovery).not.toHaveBeenCalled();
  expect(api.openFile).not.toHaveBeenCalled();
});

it("retires a preview that arrives after the dialog unmounts", async () => {
  let finish!: (preview:unknown) => void;
  api.prepareRecovery.mockImplementation(() => new Promise(resolve => {finish=resolve;}));
  const view = render(<RecoveryDialog onDone={vi.fn()}/>);
  await waitFor(() => expect(api.prepareRecovery).toHaveBeenCalledTimes(1));
  view.unmount();
  await act(async () => finish({previewId:"late-preview", before:"disk", after:"unsaved"}));
  expect(api.cancelRecoveryPreview).toHaveBeenCalledWith("late-preview");
  expect(api.applyRecoveryPreview).not.toHaveBeenCalled();
  expect(api.discardRecovery).not.toHaveBeenCalled();
});
