import {afterEach,beforeEach,expect,it,vi} from "vitest";
import {cleanup,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {assertNoA11yViolations} from "@devbox/a11y/testing";
import LegacyWorkspace from "./LegacyWorkspace";
import {nativeCall} from "./native";
vi.mock("./native",()=>({nativeCall:vi.fn()}));
const call=vi.mocked(nativeCall);
afterEach(cleanup);
beforeEach(()=>call.mockReset());
it("reads only snapshot metadata and sends the native job ID on explicit review",async()=>{
  call.mockResolvedValue({path:"C:\\offline\\한글 폴더",target:"windows"});
  const review=vi.fn();
  const {container}=render(<LegacyWorkspace jobId="native-job" disabled={false} onReview={review}/>);
  const button=await screen.findByRole("button",{name:"마지막 작업 폴더 등록 검토"});
  expect(call).toHaveBeenCalledExactlyOnceWith("workspace.migration","legacy_workspace",{jobId:"native-job"});
  expect(review).not.toHaveBeenCalled();
  fireEvent.click(button);
  expect(review).toHaveBeenCalledExactlyOnceWith("native-job");
  await assertNoA11yViolations(container);
});
it("preserves a WSL path without a Windows registration action",async()=>{
  call.mockResolvedValue({path:"\\\\wsl.localhost\\Missing\\home\\fixture",target:"wsl"});
  render(<LegacyWorkspace jobId="native-job" disabled={false} onReview={vi.fn()}/>);
  await screen.findByText(/WSL 폴더는 보관/);
  expect(screen.queryByRole("button")).toBeNull();
  expect(call).toHaveBeenCalledTimes(1);
});
it("ignores late metadata from a previous snapshot and disables reviews while editing",async()=>{
  let resolve!:(value:unknown)=>void;
  call.mockImplementation(async(_component,_method,args)=>args?.jobId==="old"?new Promise(done=>{resolve=done;}):{path:"C:\\new",target:"windows"});
  const view=render(<LegacyWorkspace jobId="old" disabled={false} onReview={vi.fn()}/>);
  view.rerender(<LegacyWorkspace jobId="new" disabled onReview={vi.fn()}/>);
  await screen.findByText("C:\\new");
  resolve({path:"C:\\old",target:"windows"});
  await waitFor(()=>expect(screen.queryByText("C:\\old")).toBeNull());
  expect(screen.getByRole("button").hasAttribute("disabled")).toBe(true);
});
