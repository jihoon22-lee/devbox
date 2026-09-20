import {afterEach,expect,it,vi} from "vitest";
import {cleanup,fireEvent,render,screen} from "@testing-library/react";
import {invoke} from "@tauri-apps/api/core";
import {fixtureDescription} from "@devbox/product-shell/api";
import catalog from "../../../apps/products.json";
import LegacyCleanup from "./LegacyCleanup";
vi.mock("@tauri-apps/api/core",()=>({invoke:vi.fn()}));
afterEach(()=>{cleanup();vi.resetAllMocks();});
const snapshot={manager:[{app:"port-manager",version:"0.4.0",mode:"portable"}],installers:null};
it("keeps cleanup unavailable until the Suite is committed",()=>{
 render(<LegacyCleanup description={fixtureDescription("control-center")} route="migration" snapshot={snapshot} onCompleted={()=>{}}/>);
 expect(screen.queryByRole("button")).toBeNull();expect(invoke).not.toHaveBeenCalled();
});
it("previews without removing and exposes partial cleanup as pending",async()=>{
 const id="cd34bbfa-23f0-40bd-83df-533d7e857171",onCompleted=vi.fn();
 vi.mocked(invoke).mockImplementation(async(_command,args)=>{
  const {request}=args as {request:{header:{requestId:string};method:string}};
  return {operation:{provenance:{product:"control-center",component:"control-center.delivery",requestId:request.header.requestId,revision:catalog.catalogRevision},outcome:{state:"succeeded"}},value:{id,app:"port-manager",version:"0.4.0",state:request.method==="legacy_cleanup_preview"?"reviewed":"cleanupPending",issue:null,preservesUserData:true}} as never;
 });
 render(<LegacyCleanup description={{...fixtureDescription("control-center"),deliveryState:"committed"}} route="migration" snapshot={snapshot} onCompleted={onCompleted}/>);
 fireEvent.click(screen.getByRole("button",{name:"이 휴대용 설치 정리 검토"}));
 const apply=await screen.findByRole("button",{name:"선택한 구 설치 정리 실행"});expect((apply as HTMLButtonElement).disabled).toBe(true);expect(invoke).toHaveBeenCalledTimes(1);
 fireEvent.click(screen.getByRole("checkbox"));fireEvent.click(app);
 await screen.findByRole("alert");expect(onCompleted).not.toHaveBeenCalled();
 expect(invoke).toHaveBeenLastCalledWith("plugin:control-center|execute",expect.objectContaining({request:expect.objectContaining({method:"legacy_cleanup_apply",args:{id}})}));
});
