export interface OutputBatch {
  frames: {sequence:number;data:string}[];
  cursor:number;
  truncated:boolean;
  closed:boolean;
  more:boolean;
}

/** One request and one xterm write at a time. Unmount drops only this subscriber. */
export function followTerminalOutput(
  read:(after:number)=>Promise<OutputBatch>,
  write:(text:string,truncated:boolean)=>Promise<void>,
  closed:()=>void,
  failed:()=>void,
):()=>void {
  let disposed=false;
  let timer:ReturnType<typeof setTimeout>|undefined;
  let wake:(()=>void)|undefined;
  const delay=()=>new Promise<void>(resolve=>{wake=resolve;timer=setTimeout(resolve,50);});
  void (async()=>{
    let cursor=0;
    try {
      while(!disposed) {
        const batch=await read(cursor);
        if(disposed)return;
        if(!Number.isSafeInteger(batch.cursor)||batch.cursor<cursor||batch.frames.length>512
          ||typeof batch.truncated!=="boolean"||typeof batch.closed!=="boolean"||typeof batch.more!=="boolean")throw new Error("invalid output");
        let previous=cursor;
        let bytes=0;
        for(const frame of batch.frames) {
          if(!Number.isSafeInteger(frame.sequence)||frame.sequence<=previous||frame.sequence>batch.cursor
            ||(previous!==cursor||!batch.truncated)&&frame.sequence!==previous+1||typeof frame.data!=="string")throw new Error("invalid output");
          previous=frame.sequence;
          bytes+=new TextEncoder().encode(frame.data).length;
        }
        if(bytes>64*1024||(batch.frames.length>0&&previous!==batch.cursor)
          ||(batch.frames.length===0&&batch.cursor!==cursor)||batch.more&&batch.cursor===cursor)throw new Error("invalid output");
        if(batch.frames.length||batch.truncated) {
          await write(batch.frames.map(frame=>frame.data).join(""),batch.truncated);
          if(disposed)return;
        }
        cursor=batch.cursor;
        if(batch.closed){closed();return;}
        if(!batch.more)await delay();
      }
    } catch {
      if(!disposed)failed();
    }
  })();
  return ()=>{disposed=true;clearTimeout(timer);wake?.();};
}
