// Hosted fixtures observe one bounded Channel batch without retaining a polling API.
export function terminalOutputExpression(sessionId, after = 0, waitMs = 1000) {
  return `(async()=>{
    const bridge=window.__TAURI_INTERNALS__,invoke=bridge.invoke;
    const d=await invoke('plugin:workspace|terminal_describe');
    const header=()=>({protocolVersion:1,installationId:d.handshake.installationId,sessionId:d.handshake.sessionId,requestId:crypto.randomUUID(),deadlineMs:Date.now()+29000,route:'terminal',context:d.context});
    let resolveBatch,rejectBatch,timer,subscription;
    const message=new Promise((resolve,reject)=>{resolveBatch=resolve;rejectBatch=reject;});
    void message.catch(()=>{});
    const callback=bridge.transformCallback(raw=>{
      if('message' in raw && raw.index===0) resolveBatch(raw.message);
      else if('end' in raw) rejectBatch(new Error('output stream ended before a batch'));
      else rejectBatch(new Error('invalid output channel sequence'));
    },false);
    try {
      subscription=await invoke('plugin:workspace|terminal_output_stream',{request:{header:header(),method:'subscribe',args:{sessionId:${JSON.stringify(sessionId)},after:${JSON.stringify(after)}}},channel:'__CHANNEL__:'+callback});
      const idle=new Promise(resolve=>{timer=setTimeout(()=>resolve({frames:[],cursor:${JSON.stringify(after)},truncated:false,closed:false,more:false}),${JSON.stringify(waitMs)});});
      return await Promise.race([message,idle]);
    } finally {
      clearTimeout(timer);
      if(subscription) await invoke('plugin:workspace|terminal_execute',{request:{header:header(),method:'unsubscribe_terminal_output',args:{subscriptionId:subscription.subscriptionId}}}).catch(()=>{});
      bridge.unregisterCallback(callback);
    }
  })()`;
}
