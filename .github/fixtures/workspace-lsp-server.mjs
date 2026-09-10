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
      if(mode!=="hang-initialize")send(message.id,{capabilities:{textDocumentSync:{openClose:true,change:1,save:true},hoverProvider:true,renameProvider:true,completionProvider:{},definitionProvider:true,referencesProvider:true,documentFormattingProvider:true,diagnosticProvider:{interFileDependencies:false,workspaceDiagnostics:false}},serverInfo:{name:"Workspace fixture",version:"1"}});
    }else if(message.method==="textDocument/hover"){
      const reply=()=>send(message.id,{contents:{kind:"plaintext",value:"fixture hover"}});
      if(mode==="documents"&&message.params.position.character===2){
        writeFileSync(marker+".hover","pending");
        setTimeout(reply,750);
      }else reply();
    }
    else if(message.method==="textDocument/rename"){
      const uri=message.params.textDocument.uri;
      const edit={range:{start:{line:0,character:4},end:{line:0,character:9}},newText:message.params.newName};
      send(message.id,{changes:{[uri]:[edit],[new URL("notes.txt",uri).href]:[edit]}});
    }
    else if(message.method==="textDocument/completion")send(message.id,[{label:"fixture",kind:6}]);
    else if(message.method==="textDocument/diagnostic")send(message.id,{kind:"full",items:[]});
    else if(message.method==="textDocument/definition"||message.method==="textDocument/references"||message.method==="textDocument/formatting")send(message.id,[]);
    else if(message.method==="shutdown")send(message.id,null);
    else if(message.method==="exit")process.exit(0);
    else if(message.id!==undefined)send(message.id,null);
  }
});
// An assertion failure must not leave a detached fixture indefinitely.
setTimeout(()=>process.exit(3),60_000).unref();
