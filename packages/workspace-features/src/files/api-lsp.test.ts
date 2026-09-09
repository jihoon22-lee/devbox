import {beforeEach,expect,it,vi} from "vitest";
const mocks=vi.hoisted(()=>({invoke:vi.fn(),hosted:true}));
vi.mock("../transport",()=>({componentInvoke:()=>mocks.invoke,isProductHosted:()=>mocks.hosted}));
import {startLanguageServer,restartLanguageServer,stopLanguageServer} from "./api";
beforeEach(()=>{mocks.hosted=true;mocks.invoke.mockReset().mockResolvedValue(undefined);});

it("cancels a native start even before its first invocation and uses a fresh retry ID",async()=>{
  let finish!:()=>void;
  const pending=new Promise<void>(resolve=>{finish=resolve;});
  mocks.invoke.mockImplementation((method:string)=>method==="start_language_server"?pending:Promise.resolve());
  const starting=startLanguageServer("rust");
  expect(startLanguageServer("rust")).toBe(starting);
  await stopLanguageServer("rust");
  const stop=mocks.invoke.mock.calls.find(([method])=>method==="stop_language_server")!;
  const start=mocks.invoke.mock.calls.find(([method])=>method==="start_language_server")!;
  expect(stop[1].operationId).toBe(start[1].operationId);
  expect(start[1].operationId).toMatch(/^[a-f0-9-]{36}$/u);
  finish();await starting;
  await restartLanguageServer("rust");
  const retry=mocks.invoke.mock.calls.find(([method])=>method==="restart_language_server")!;
  expect(retry[1].operationId).not.toBe(start[1].operationId);
});

it("retains the standalone language-server arguments",async()=>{
  mocks.hosted=false;
  await startLanguageServer("rust");await stopLanguageServer("rust");
  expect(mocks.invoke).toHaveBeenNthCalledWith(1,"start_language_server",{languageId:"rust"});
  expect(mocks.invoke).toHaveBeenNthCalledWith(2,"stop_language_server",{languageId:"rust"});
});
