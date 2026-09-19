import {afterEach,expect,it,vi} from "vitest";
import {cleanup,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {fixtureDescription} from "@devbox/product-shell/api";
import {searchCommands,searchCommandSource,previewCommand,type CommandSearch} from "@devbox/product-shell/commands";
import Commands from "./Commands";
vi.mock("@devbox/product-shell/commands",()=>({onConnectionChanged:vi.fn(async()=>()=>{}),searchCommands:vi.fn(),searchCommandSource:vi.fn().mockRejectedValue(new Error("unavailable")),previewCommand:vi.fn(),openCommand:vi.fn(),commandStatus:vi.fn()}));
afterEach(()=>{cleanup();vi.clearAllMocks();});
const description=fixtureDescription("control-center");
const item={id:"control-center.open-tools",owner:"control-center",component:"control-center.shell",
 label:"도구",revision:"a".repeat(64),target:{kind:"route" as const,route:"tools"},
 requiredContext:"none" as const,context:null,destructive:false,requiresReview:false,disabledReason:null};
it("ignores an older search response and opens only the current native-reviewed route",async()=>{
 let firstResolve!:(value:CommandSearch)=>void;
 vi.mocked(searchCommands).mockImplementation(async(_description,_route,query)=>query?
   {results:[item],truncated:false}:new Promise(resolve=>{firstResolve=resolve;}));
 vi.mocked(previewCommand).mockResolvedValue(item);
 const navigate=vi.fn();
 render(<Commands description={description} route="products" navigate={navigate} refreshContext={async()=>{}}/>);
 await waitFor(()=>expect(firstResolve).toBeDefined());
 fireEvent.change(screen.getByRole("searchbox"),{target:{value:"도구"}});
 const button=await screen.findByRole("button",{name:"도구"});
 firstResolve({results:[],truncated:false});
 await waitFor(()=>expect((button as HTMLButtonElement).disabled).toBe(false));
 fireEvent.click(button);
 await waitFor(()=>expect(navigate).toHaveBeenCalledWith("tools"));
 expect(previewCommand).toHaveBeenCalledWith(description,"products",item,expect.any(String));
});
it("keeps unavailable owners visible without invoking them",async()=>{
 vi.mocked(searchCommands).mockResolvedValue({results:[{...item,id:"workspace.open-files",owner:"workspace",component:"workspace.shell",label:"파일",target:{kind:"route",route:"files"},disabledReason:"providerUnavailable"}],truncated:false});
 render(<Commands description={description} route="products" navigate={vi.fn()} refreshContext={async()=>{}}/>);
 expect((await screen.findByRole("button",{name:"파일"}) as HTMLButtonElement).disabled).toBe(true);
 expect(screen.getByText(/제품 연결 확인 필요/)).toBeTruthy();
 expect(previewCommand).not.toHaveBeenCalled();
});

it("shows an available provider while another is pending and rejects late generations",async()=>{
 vi.mocked(searchCommands).mockResolvedValue({results:[item],truncated:false});
 let oldResolve!:(value:CommandSearch)=>void;
 vi.mocked(searchCommandSource).mockImplementation(async(_description,_route,product,query)=>{
  if(product==="workspace")return new Promise(()=>{});
  if(product==="knowledge"&&!query)return new Promise(resolve=>{oldResolve=resolve;});
  if(product==="knowledge")return {results:[{...item,id:"knowledge.open-notes",owner:"knowledge",component:"knowledge.shell",label:"새 노트",target:{kind:"route",route:"notes"}}],truncated:false};
  throw new Error("offline");
 });
 render(<Commands description={description} route="products" navigate={vi.fn()} refreshContext={async()=>{}}/>);
 await waitFor(()=>expect(oldResolve).toBeDefined());
 expect(screen.getByRole("button",{name:"도구"})).toBeTruthy();
 fireEvent.change(screen.getByRole("searchbox"),{target:{value:"새"}});
 expect(await screen.findByRole("button",{name:"새 노트"})).toBeTruthy();
 oldResolve({results:[],truncated:false});
 await waitFor(()=>expect(screen.getByRole("button",{name:"새 노트"})).toBeTruthy());
});
