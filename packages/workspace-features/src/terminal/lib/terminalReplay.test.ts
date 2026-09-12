import { describe, expect, it, vi } from "vitest";
import { followTerminalOutput, type OutputBatch } from "./terminalReplay";

const batch=(sequence:number,data:string,closed=false):OutputBatch=>({frames:[{sequence,data}],cursor:sequence,truncated:false,closed,more:!closed});
const settle=async()=>{for(let index=0;index<6;index+=1)await Promise.resolve();};

describe("owned terminal replay",()=>{
  it("waits for xterm before reading more and reports closure only after final drain",async()=>{
    let drained:(()=>void)|undefined;
    const read=vi.fn().mockResolvedValueOnce(batch(1,"first")).mockResolvedValueOnce(batch(2,"last",true));
    const write=vi.fn().mockImplementationOnce(()=>new Promise<void>(resolve=>{drained=resolve;})).mockResolvedValue(undefined);
    const closed=vi.fn();const failed=vi.fn();
    const stop=followTerminalOutput(read,write,closed,failed);
    await settle();expect(read).toHaveBeenCalledTimes(1);expect(closed).not.toHaveBeenCalled();
    drained?.();await settle();expect(read).toHaveBeenLastCalledWith(1);expect(write).toHaveBeenLastCalledWith("last",false);
    expect(closed).toHaveBeenCalledTimes(1);expect(failed).not.toHaveBeenCalled();stop();
  });
  it("a detached renderer never consumes a late response or closes a PTY",async()=>{
    let complete:((value:OutputBatch)=>void)|undefined;
    const read=vi.fn(()=>new Promise<OutputBatch>(resolve=>{complete=resolve;}));
    const write=vi.fn();const closed=vi.fn();const failed=vi.fn();
    const stop=followTerminalOutput(read,write,closed,failed);stop();
    complete?.(batch(1,"late",true));await settle();
    expect(write).not.toHaveBeenCalled();expect(closed).not.toHaveBeenCalled();expect(failed).not.toHaveBeenCalled();expect(read).toHaveBeenCalledTimes(1);
  });
  it("requires an explicit loss marker for skipped output sequences",async()=>{
    const write=vi.fn().mockResolvedValue(undefined);const closed=vi.fn();const failed=vi.fn();
    const invalid=followTerminalOutput(async()=>batch(5,"missing",true),write,closed,failed);
    await settle();expect(failed).toHaveBeenCalledTimes(1);expect(write).not.toHaveBeenCalled();invalid();
    const valid=followTerminalOutput(async()=>({...batch(5,"retained",true),truncated:true}),write,closed,failed);
    await settle();expect(write).toHaveBeenCalledWith("retained",true);expect(closed).toHaveBeenCalledTimes(1);valid();
  });
});
