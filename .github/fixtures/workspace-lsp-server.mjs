// Synthetic stdio peer for native Workspace ownership and IPC fixtures.
// No user files, network requests or package installation participate.
import {writeFileSync} from "node:fs";
const [marker,mode]=process.argv.slice(2);
if(marker)writeFileSync(marker,String(process.pid));
let buffer=Buffer.alloc(0);
function send(id,result){const body=Buffer.from(JSON.stringify({jsonrpc:"2.0",id,result}));process.stdout.write(`Content-Length: ${body.length}\r\n\r\n`);process.stdout.write(body);}
process.stdin.on("data",chunk=>{
  buffer=Buffer.concat([buffer,chunk]);
  while(true){
    const boundary=buffer.indexOf("\r\n\r\n");if(boundary<0)return;
    const length=/Content-Length:\s*(\d+)/iu.exec(buffer.subarray(0,boundary).toString())?.[1];
    if(!length)process.exit(2);
    const end=boundary+4+Number(length);if(buffer.length<end)return;
    const message=JSON.parse(buffer.subarray(boundary+4,end));buffer=buffer.subarray(end);
    if(message.method==="initialize"){
      if(mode!=="hang-initialize")send(message.id,{capabilities:{textDocumentSync:1},serverInfo:{name:"Workspace fixture",version:"1"}});
    }else if(message.method==="shutdown")send(message.id,null);
    else if(message.method==="exit")process.exit(0);
    else if(message.id!==undefined)send(message.id,null);
  }
});
// An assertion failure must not leave a detached fixture indefinitely.
setTimeout(()=>process.exit(3),20_000).unref();
