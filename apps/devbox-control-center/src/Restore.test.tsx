import {afterEach, expect, it, vi} from "vitest";
import {cleanup, fireEvent, render, screen, waitFor} from "@testing-library/react";
import {invoke} from "@tauri-apps/api/core";
import {fixtureDescription} from "@devbox/product-shell/api";
import catalog from "../../../apps/products.json";
import Restore from "./Restore";
vi.mock("@tauri-apps/api/core",()=>({invoke:vi.fn(),isTauri:()=>true}));
vi.mock("@devbox/product-shell/api",async original=>({...await original<typeof import("@devbox/product-shell/api")>(),nativeMode:true}));
afterEach(()=>{cleanup();vi.resetAllMocks();});
const checkpoint="fa2c49ba-1368-4c79-aafb-d7a6c7ef6c14";
function respond(activeOperation:string|null=null) {
 vi.mocked(invoke).mockImplementation(async(_command,args)=>{
  const {request}=args as {request:{header:{requestId:string};method:string}};
  return {operation:{provenance:{product:"control-center",component:"control-center.delivery",requestId:request.header.requestId,revision:catalog.catalogRevision},outcome:{state:"succeeded"}},
   value:request.method==="restore_inventory"?{checkpoints:[{id:checkpoint,bytes:512,files:2}],operations:activeOperation?[{id:activeOperation,phase:"health"}]:[],activeOperation}:{accepted:true}} as never;
 });
}
it("requires explicit review before closing the shell and sends only the selected checkpoint id",async()=>{
 respond();render(<Restore description={fixtureDescription("control-center")} route="recovery"/>);
 fireEvent.click(await screen.findByRole("button",{name:"이 보존본으로 복원"}));
 const execute=screen.getByRole("button",{name:"Control Center를 닫고 실행"});
 expect((execute as HTMLButtonElement).disabled).toBe(true);
 expect(invoke).toHaveBeenCalledTimes(1);
 fireEvent.click(screen.getByRole("checkbox"));fireEvent.click(execute);
 await screen.findByText(/복구 도우미를 시작했습니다/);
 expect(invoke).toHaveBeenLastCalledWith("plugin:control-center|execute",expect.objectContaining({request:expect.objectContaining({method:"restore_action",args:{action:"restore",id:checkpoint}})}));
 expect(invoke).toHaveBeenCalledTimes(2);
});
it("keeps a pending restore visible and prevents starting a second snapshot or restore",async()=>{
 respond(checkpoint);render(<Restore description={fixtureDescription("control-center")} route="recovery"/>);
 expect((await screen.findByRole("button",{name:"이 보존본으로 복원"}) as HTMLButtonElement).disabled).toBe(true);
 expect((screen.getByRole("button",{name:"현재 데이터 보존"}) as HTMLButtonElement).disabled).toBe(true);
 fireEvent.click(screen.getByRole("button",{name:"원본으로 복귀"}));
 fireEvent.click(screen.getByRole("checkbox"));fireEvent.click(screen.getByRole("button",{name:"Control Center를 닫고 실행"}));
 await waitFor(()=>expect(invoke).toHaveBeenCalledTimes(2));
 expect(invoke).toHaveBeenLastCalledWith("plugin:control-center|execute",expect.objectContaining({request:expect.objectContaining({method:"restore_action",args:{action:"rollback",id:checkpoint}})}));
});
