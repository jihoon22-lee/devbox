import {afterEach,beforeEach,expect,it,vi} from "vitest";
import {cleanup,render,screen,waitFor} from "@testing-library/react";
import {IncomingReviewContext,type IncomingReview} from "@devbox/product-shell/incoming";
import {openReceivedFile} from "@devbox/product-shell/commands";
import type {Description,ProjectContext} from "@devbox/product-shell/api";
import IncomingFileReview from "./IncomingFileReview";
vi.mock("@devbox/product-shell/commands",()=>({openReceivedFile:vi.fn()}));
afterEach(cleanup);
beforeEach(()=>vi.mocked(openReceivedFile).mockReset());
const context:ProjectContext={projectId:"project-1",worktreeId:"tree-1",target:{kind:"windows"},revision:1};
const review:IncomingReview={operationId:"review-1",revision:"a".repeat(64),commandRevision:"b".repeat(64),label:"Selected file",route:"files",context:null,target:{kind:"entity",entity:"file",id:"reference-1"}};
function mount(context:ProjectContext|null,onOpen:ReturnType<typeof vi.fn>){
 return render(<IncomingReviewContext.Provider value={{review,clear:()=>{}}}><IncomingFileReview description={{context} as Description} onOpen={onOpen}/></IncomingReviewContext.Provider>);
}
it("accepts the same native context with reordered JSON keys and uses the editor's context key",async()=>{
 const reordered:ProjectContext={revision:1,target:{kind:"windows"},worktreeId:"tree-1",projectId:"project-1"};
 vi.mocked(openReceivedFile).mockResolvedValue({path:"C:\\fixture\\selected.txt",context:reordered});
 const open=vi.fn();mount(context,open);
 await waitFor(()=>expect(open).toHaveBeenCalledTimes(1));
 expect(open).toHaveBeenCalledWith({id:"review-1",contextKey:JSON.stringify(context),path:"C:\\fixture\\selected.txt",line:null,receivedReference:"reference-1"});
 expect(screen.queryByRole("alert")).toBeNull();
});
it("still rejects a changed native revision",async()=>{
 vi.mocked(openReceivedFile).mockResolvedValue({path:"C:\\fixture\\selected.txt",context:{...context,revision:2}});
 const open=vi.fn();mount(context,open);
 await screen.findByRole("alert");expect(open).not.toHaveBeenCalled();
});
it("opens an approved standalone file without selecting a project",async()=>{
 vi.mocked(openReceivedFile).mockResolvedValue({path:"C:\\fixture\\selected.txt",context:null});
 const open=vi.fn();mount(null,open);
 await waitFor(()=>expect(open).toHaveBeenCalledTimes(1));
 expect(open.mock.calls[0][0].contextKey).toBe("null");
});
