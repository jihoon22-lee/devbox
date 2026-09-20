import {afterEach,expect,it,vi} from "vitest";
import {cleanup,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {invoke} from "@tauri-apps/api/core";
import {fixtureDescription} from "@devbox/product-shell/api";
import catalog from "../../../apps/products.json";
import Inventory from "./Inventory";
vi.mock("@tauri-apps/api/core",()=>({invoke:vi.fn(),isTauri:()=>true}));
vi.mock("@devbox/product-shell/api",async original=>({...await original<typeof import("@devbox/product-shell/api")>(),nativeMode:true}));
afterEach(()=>{cleanup();vi.resetAllMocks();});
function respond(declaration="verified",foreignFolder=false){
 vi.mocked(invoke).mockImplementation(async(_command,args)=>{
  const {request}=args as {request:{header:{requestId:string};method:string}};
  const folder=request.method==="open_installation_folder";
  return {operation:{provenance:{product:folder&&foreignFolder?"workspace":"control-center",component:"control-center.delivery",requestId:request.header.requestId,revision:catalog.catalogRevision},outcome:{state:"succeeded"}},value:folder?{opened:true}:{installationKey:"a".repeat(64),generation:"g1",suiteVersion:"0.8.0",declaration,installerRegistration:"verified",products:[],components:[]}} as never;
 });
}
it("opens the native installation directory only after a click without sending a path",async()=>{
 respond();render(<Inventory description={fixtureDescription("control-center")} route="products" navigate={vi.fn()} refreshContext={async()=>{}}/>);
 const button=screen.getByRole("button",{name:"설치 폴더 열기"}) as HTMLButtonElement;
 await waitFor(()=>expect(button.disabled).toBe(false));expect(invoke).toHaveBeenCalledTimes(1);
 fireEvent.click(button);
 await waitFor(()=>expect(invoke).toHaveBeenCalledTimes(2));
 expect(invoke).toHaveBeenLastCalledWith("plugin:control-center|execute",expect.objectContaining({request:expect.objectContaining({method:"open_installation_folder",args:{}})}));
});
it("keeps unknown installations from exposing an enabled folder action",async()=>{
 respond("unknown");render(<Inventory description={fixtureDescription("control-center")} route="products" navigate={vi.fn()} refreshContext={async()=>{}}/>);
 await screen.findByText(/Suite 0.8.0/);
 const button=screen.getByRole("button",{name:"설치 폴더 열기"}) as HTMLButtonElement;
 expect(button.disabled).toBe(true);fireEvent.click(button);expect(invoke).toHaveBeenCalledTimes(1);
});
it("rejects a folder result from another native owner",async()=>{
 respond("verified",true);render(<Inventory description={fixtureDescription("control-center")} route="products" navigate={vi.fn()} refreshContext={async()=>{}}/>);
 const button=screen.getByRole("button",{name:"설치 폴더 열기"}) as HTMLButtonElement;
 await waitFor(()=>expect(button.disabled).toBe(false));fireEvent.click(button);
 await screen.findByRole("alert");
});
