import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn(), listeners: new Map<string,()=>void>() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke, isTauri: () => true }));

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async (event:string, callback:()=>void) => { tauri.listeners.set(event,callback); return () => tauri.listeners.delete(event); }) }));
vi.mock("./ShortcutSettings", () => ({ default: () => <div>Shortcut settings</div> }));
import SuiteConnection from "./SuiteConnection";
import { fixtureDescription } from "./api";
import catalog from "../../../apps/products.json";

const description = fixtureDescription("workspace");
type ConnectionArgs = { request: { header: { requestId: string }; method: { kind: string } } };
function reply(value: unknown, args: ConnectionArgs) {
  const requestId = args.request.header.requestId;
  return { operation: { provenance: { product: "workspace", component: "workspace.commands", requestId, revision: catalog.catalogRevision }, outcome: { state: "succeeded" } }, value };
}
beforeEach(() => { tauri.invoke.mockReset(); tauri.listeners.clear(); });
afterEach(cleanup);

it("shows the automatic connection and turns it off", async () => {
  tauri.invoke.mockImplementation(async (_cmd: string, args: ConnectionArgs) => {
    const value = args.request.method.kind === "disconnect"
      ? { connected: false, generation: null, mode: "off", issue: null }
      : { connected: true, generation: "g1", mode: "auto", issue: null };
    return reply(value, args);
  });
  const {container}=render(<SuiteConnection description={description} route="overview" />);
  expect(await screen.findByText("이 설치의 제품이 연결되어 있습니다.")).toBeTruthy();
  await assertNoA11yViolations(container);
  fireEvent.click(screen.getByRole("button", { name: "자동 연결 끄기" }));
  expect(await screen.findByText("자동 연결이 꺼져 있습니다.")).toBeTruthy();
});

it("explains why a development build cannot connect", async () => {
  tauri.invoke.mockImplementation(async (_cmd: string, args: ConnectionArgs) =>
    reply({ connected: false, generation: null, mode: "auto", issue: "suite_package_unavailable" }, args));
  render(<SuiteConnection description={description} route="overview" />);
  await waitFor(() => expect(screen.getByText(/설치형 Suite로 실행할 때만/)).toBeTruthy());
});

it("refreshes after startup automatic connection completes", async () => {
  let connected=false;
  tauri.invoke.mockImplementation(async (_cmd:string,args:ConnectionArgs)=>reply({connected,generation:connected?"g1":null,mode:"auto",issue:null},args));
  render(<SuiteConnection description={description} route="overview" />);
  await waitFor(()=>expect(tauri.invoke).toHaveBeenCalled());
  connected=true;
  tauri.listeners.get("suite-connection-status")?.();
  expect(await screen.findByText("이 설치의 제품이 연결되어 있습니다.")).toBeTruthy();
});

it("does not let a delayed status overwrite a manual disconnect", async () => {
  const connected={connected:true,generation:"g1",mode:"auto",issue:null};
  let complete:(()=>void)|undefined;
  tauri.invoke.mockImplementationOnce(async (_cmd:string,args:ConnectionArgs)=>reply(connected,args));
  tauri.invoke.mockImplementation(async (_cmd:string,args:ConnectionArgs)=>{
    if(args.request.method.kind==="disconnect")return reply({connected:false,generation:null,mode:"off",issue:null},args);
    return new Promise(resolve=>{complete=()=>resolve(reply(connected,args));});
  });
  render(<SuiteConnection description={description} route="overview" />);
  await screen.findByText("이 설치의 제품이 연결되어 있습니다.");
  act(()=>tauri.listeners.get("suite-connection-status")?.());
  await waitFor(()=>expect(complete).toBeTypeOf("function"));
  fireEvent.click(screen.getByRole("button",{name:"자동 연결 끄기"}));
  await screen.findByText("자동 연결이 꺼져 있습니다.");
  await act(async()=>{complete!();});
  expect(screen.getByText("자동 연결이 꺼져 있습니다.")).toBeTruthy();
});
