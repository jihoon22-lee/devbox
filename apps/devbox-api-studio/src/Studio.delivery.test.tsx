import {act,cleanup,render,screen} from "@testing-library/react";
import {afterEach,beforeEach,expect,it,vi} from "vitest";
import type {ReactNode} from "react";
import type {ShellContentProps} from "@devbox/product-shell";
import type {Description} from "@devbox/product-shell/api";
import Studio from "./Studio";
const mode=vi.hoisted(()=>({phase:"import" as NonNullable<Description["deliveryState"]>}));
vi.mock("@devbox/product-shell",async()=>{
 const {fixtureDescription}=await import("@devbox/product-shell/api");
 return {ProductShell:({renderContent}:{renderContent:(props:ShellContentProps)=>ReactNode})=>renderContent({description:{...fixtureDescription("api-studio"),deliveryState:mode.phase},route:"requests",navigate:()=>{},refreshContext:async()=>{}})};
});
vi.mock("./migration/Startup",()=>({MigrationStartup:({children}:{children:ReactNode})=><>{children}</>}));
vi.mock("@devbox/api-studio-features/requests",async()=>{
 const {useEffect}=await import("react");
 return {default:()=>{useEffect(()=>{localStorage.setItem("delivery-writer","mounted");},[]);return <div>Business request view</div>;}};
});
beforeEach(()=>localStorage.setItem("delivery-writer","preserved"));
afterEach(()=>{cleanup();localStorage.clear();});
it.each(["import","health","recover","unavailable"] as const)("keeps browser-storage writers unmounted during %s",async phase=>{
 mode.phase=phase;render(<Studio/>);
 await act(async()=>{await vi.dynamicImportSettled();});
 expect(screen.queryByText("Business request view")).toBeNull();
 expect(localStorage.getItem("delivery-writer")).toBe("preserved");
});
it("mounts the ordinary view after committed activation",async()=>{
 mode.phase="committed";render(<Studio/>);
 await screen.findByText("Business request view");
 expect(localStorage.getItem("delivery-writer")).toBe("mounted");
});
