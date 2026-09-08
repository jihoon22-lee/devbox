import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
const mocks=vi.hoisted(()=>({rpc:vi.fn(),job:vi.fn()}));
vi.mock("./vaultApi",()=>({vaultInvoke:mocks.rpc,vaultJob:mocks.job}));
import VaultSetup from "./VaultSetup";
beforeEach(()=>{
  mocks.rpc.mockReset().mockImplementation(async(method:string)=>method==="continue_existing"?{active:true}:{schedule:{id:"plan",target:"C:/new",previousRoot:"C:/old"}});
  mocks.job.mockReset().mockImplementation(async(method:string)=>method==="apply_vault_change"?{active:true}:{previewId:"native-preview",target:"C:/new",previousRoot:"C:/old",alreadyApplied:false,expiresInSeconds:300});
});
afterEach(cleanup);
async function preview(){
  const button=screen.getByRole("button",{name:"폴더 연결 미리보기"});
  await waitFor(()=>expect((button as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(button);
  await screen.findByRole("heading",{name:"연결 미리보기"});
}
it("preview and cancellation leave the current folder as the only activated store",async()=>{
  const active=vi.fn(); render(<VaultSetup onActivated={active}/>); await preview();
  expect(active).not.toHaveBeenCalled();
  expect(mocks.job.mock.calls.map(([method])=>method)).toEqual(["prepare_vault_change"]);
  fireEvent.click(screen.getByRole("button",{name:"현재 폴더 유지하고 계속"}));
  await waitFor(()=>expect(active).toHaveBeenCalledOnce());
  expect(mocks.rpc.mock.calls.map(([method])=>method)).toEqual(["vault_change_status","cancel_vault_change","continue_existing"]);
});
it("applies only the native preview token after a separate explicit action",async()=>{
  const active=vi.fn(); render(<VaultSetup onActivated={active}/>); await preview();
  fireEvent.click(screen.getByRole("button",{name:"이 폴더로 변경하고 시작"}));
  await waitFor(()=>expect(active).toHaveBeenCalledOnce());
  expect(mocks.job.mock.calls[1][0]).toBe("apply_vault_change");
  expect(mocks.job.mock.calls[1][1]).toEqual({id:"native-preview"});
});
