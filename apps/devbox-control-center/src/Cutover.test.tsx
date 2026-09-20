import {afterEach,expect,it,vi} from "vitest";
import {cleanup,fireEvent,render,screen} from "@testing-library/react";
import {invoke} from "@tauri-apps/api/core";
import {fixtureDescription} from "@devbox/product-shell/api";
import catalog from "../../../apps/products.json";
import Cutover from "./Cutover";
vi.mock("@tauri-apps/api/core",()=>({invoke:vi.fn()}));
vi.mock("@devbox/product-shell/api",async original=>({...await original<typeof import("@devbox/product-shell/api")>(),nativeMode:true}));
afterEach(()=>{cleanup();vi.resetAllMocks();});
it("requires an explicit skipped-source choice, then a separate activation confirmation",async()=>{
 const identifier="com.devbox.devboxlauncher",revision="a".repeat(64);
 vi.mocked(invoke).mockImplementation(async(_command,args)=>{
  const {request}=args as {request:{header:{requestId:string};method:string;args:unknown}};
  return {operation:{provenance:{product:"control-center",component:"control-center.delivery",requestId:request.header.requestId,revision:catalog.catalogRevision},outcome:{state:"succeeded"}},value:request.method==="restore_action"?{accepted:true}:{revision,sources:[{identifier,owner:"control-center",currentImport:false,backupCount:0}],prepared:request.method==="prepare_cutover",choices:request.method==="prepare_cutover"?[{identifier,disposition:"keepLegacy"}]:[]}} as never;
 });
 render(<Cutover description={fixtureDescription("control-center")} route="migration"/>);
 expect(invoke).not.toHaveBeenCalled();fireEvent.click(screen.getByRole("button",{name:"전환 계획 불러오기"}));
 const select=await screen.findByRole("combobox");
 expect(screen.queryByRole("option",{name:"원본이 일치하는 가져오기 결과 사용"})).toBeNull();
 fireEvent.click(screen.getByRole("checkbox"));
 const prepare=screen.getByRole("button",{name:"선택한 전환 계획 보존"});expect((prepare as HTMLButtonElement).disabled).toBe(true);
 fireEvent.change(select,{target:{value:"keepLegacy"}});fireEvent.click(screen.getByRole("checkbox"));fireEvent.click(prepare);
 const activate=await screen.findByRole("button",{name:"Control Center를 닫고 전환 준비"});
 expect((activate as HTMLButtonElement).disabled).toBe(true);expect(invoke).toHaveBeenCalledTimes(2);
 expect(invoke).toHaveBeenLastCalledWith("plugin:control-center|execute",expect.objectContaining({request:expect.objectContaining({method:"prepare_cutover",args:{revision,choices:[{identifier,disposition:"keepLegacy"}]}})}));
 fireEvent.click(screen.getByRole("checkbox"));fireEvent.click(activate);await screen.findByRole("status");
 expect(invoke).toHaveBeenCalledTimes(3);
});
