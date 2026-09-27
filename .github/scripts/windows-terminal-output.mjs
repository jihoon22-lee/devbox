import { stripVTControlCharacters } from "node:util";

export function terminalPromptVisible(output) {
  // Remove complete OSC titles first: Node's generic stripper can leave a
  // fragment of an adjacent title attached to a ConPTY prompt.
  const visible = output.replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, "");
  return /[#$](?:\s|$)/.test(stripVTControlCharacters(visible));
}

// Hosted fixtures observe one bounded Channel batch without retaining a polling API.
export function terminalOutputExpression(sessionId, after = 0, waitMs = 1000) {
  return `(async()=>{
    const bridge=window.__TAURI_INTERNALS__,invoke=bridge.invoke;
    const d=await invoke('plugin:workspace|terminal_describe'),deadline=Date.now()+29000;
    const header=()=>({protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:deadline,route:'terminal',context:d.context});
    let active,timer;
    try {
      for(let attempt=0;;attempt++){
        // Deserializing even a rejected invocation creates a native Channel.
        // Its delayed end must never share the successful retry's callback.
        let resolveBatch,rejectBatch;
        const message=new Promise((resolve,reject)=>{resolveBatch=resolve;rejectBatch=reject;});
        void message.catch(()=>{});
        const callback=bridge.transformCallback(raw=>{
          if(raw&&typeof raw==='object'&&'message' in raw&&raw.index===0) resolveBatch(raw.message);
          else if(raw&&raw.end===true&&raw.index===1) {
            // Tauri fetches large messages asynchronously; end(1) may arrive
            // before message(0). Wait for that message just like Channel does.
          }
          else if(raw&&raw.end===true&&raw.index===0) rejectBatch(new Error('output stream ended before a batch'));
          else rejectBatch(new Error('invalid output channel sequence'));
        },false);
        try{
          const subscription=await invoke('plugin:workspace|terminal_output_stream',{request:{header:header(),method:'subscribe',args:{sessionId:${JSON.stringify(sessionId)},after:${JSON.stringify(after)}}},channel:'__CHANNEL__:'+callback});
          active={subscription,callback,message};
          break;
        }catch(problem){
          bridge.unregisterCallback(callback);
          if(problem==='busy'&&attempt<19&&Date.now()+50<deadline){await new Promise(resolve=>setTimeout(resolve,50));continue;}
          throw problem;
        }
      }
      const idle=new Promise(resolve=>{timer=setTimeout(()=>resolve({frames:[],cursor:${JSON.stringify(after)},truncated:false,closed:false,more:false}),${JSON.stringify(waitMs)});});
      return await Promise.race([active.message,idle]);
    } finally {
      clearTimeout(timer);
      if(active){
        await invoke('plugin:workspace|terminal_execute',{request:{header:header(),method:'unsubscribe_terminal_output',args:{subscriptionId:active.subscription.subscriptionId}}}).catch(()=>{});
        bridge.unregisterCallback(active.callback);
      }
    }
  })()`;
}
