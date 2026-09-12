import {afterEach,beforeEach,expect,it,vi} from "vitest";
import {cleanup,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {fixtureDescription} from "@devbox/product-shell/api";
import Terminal from "./Terminal";
import {componentCall} from "./native";
vi.mock("./native",()=>({componentCall:vi.fn()}));
vi.mock("./TerminalImport",()=>({default:()=>null}));
vi.mock("./DevelopmentSessions",()=>({default:()=>null}));
const call=vi.mocked(componentCall);
const context={projectId:"project",worktreeId:"tree",target:{kind:"windows" as const},revision:1};
const description={...fixtureDescription("workspace"),context};
beforeEach(()=>{
  sessionStorage.clear();
  call.mockReset().mockImplementation(async(_description,_component,method)=>{
    if(method==="terminal_sessions")return[{id:"10000000-0000-4000-8000-000000000001",context,state:"active"}];
    if(method==="terminal_commands")return{profiles:[{id:"profile",name:"Synthetic profile",revision:"a".repeat(64)}]};
    return{};
  });
});
afterEach(cleanup);
it("opens the selected profile with its native revision and no renderer command text",async()=>{
  render(<Terminal description={description} registry={null}/>);
  await screen.findByRole("option",{name:"Synthetic profile"});
  fireEvent.change(screen.getByRole("combobox"),{target:{value:"profile"}});
  fireEvent.click(screen.getByRole("button",{name:"프로필로 터미널 열기"}));
  await waitFor(()=>expect(call.mock.calls.some(([, ,method])=>method==="open_terminal_profile")).toBe(true));
  const args=call.mock.calls.find(([, ,method])=>method==="open_terminal_profile")?.[3];
  expect(args).toEqual({operationId:expect.any(String),profileId:"profile",revision:"a".repeat(64)});
});
it("reuses the visibility operation after an ambiguous native reply",async()=>{
  const requests:Array<Record<string,unknown>>=[];
  const original=call.getMockImplementation()!;
  call.mockImplementation(async(...args)=>{
    if(args[2]==="summon_terminal"){requests.push(args[3]);throw new Error("synthetic lost reply");}
    return original(...args);
  });
  render(<Terminal description={description} registry={null}/>);
  const button=await screen.findByRole("button",{name:"창 표시·숨김"});
  fireEvent.click(button);
  await screen.findByRole("alert");
  fireEvent.click(button);
  await waitFor(()=>expect(requests).toHaveLength(2));
  expect(requests[0]).toEqual(requests[1]);
  expect(call.mock.calls.some(([, ,method])=>["open_terminal","stop_terminal"].includes(method))).toBe(false);
});
