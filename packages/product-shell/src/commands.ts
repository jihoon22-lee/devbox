import {invoke} from "@tauri-apps/api/core";
import {makeRequest,nativeMode,isProjectContext,type Description,type ProjectContext} from "./api";
import {isOperation,problemMessage} from "./operation";
import catalog from "../../../apps/products.json";

export interface Command {
  id:string;owner:string;component:string;label:string;revision:string;
  target:{kind:"route";route:string}|{kind:"entity";entity:string;id:string};
  requiredContext:"none"|"project"|"selection";context:ProjectContext|null;
  destructive:boolean;requiresReview:boolean;disabledReason:string|null;
}
export interface CommandSearch {results:Command[];truncated:boolean}
const owners=["workspace","api-studio","knowledge","control-center"];
function command(value:unknown):value is Command {
  if(!value||typeof value!=="object"||Array.isArray(value))return false;
  const row=value as Record<string,unknown>;
  if(typeof row.id!=="string"||row.id.length>256||typeof row.owner!=="string"||!owners.includes(row.owner)
    ||!row.id.startsWith(row.owner+".")||typeof row.component!=="string"||!row.component.startsWith(row.owner+".")
    ||typeof row.label!=="string"||row.label.length>256||/[\x00-\x1f\x7f]/.test(row.label)
    ||typeof row.revision!=="string"||!/^[a-f0-9]{64}$/.test(row.revision)
    ||!["none","project","selection"].includes(String(row.requiredContext))
    ||(row.context!==null&&!isProjectContext(row.context))
    ||typeof row.destructive!=="boolean"||typeof row.requiresReview!=="boolean"
    ||(row.destructive&&!row.requiresReview)
    ||(row.disabledReason!==null&&typeof row.disabledReason!=="string")
    ||!row.target||typeof row.target!=="object"||Array.isArray(row.target))return false;
  const target=row.target as Record<string,unknown>;
  return target.kind==="route"?typeof target.route==="string"&&/^[a-z0-9-]{1,96}$/.test(target.route)
    :target.kind==="entity"&&typeof target.entity==="string"&&typeof target.id==="string"&&/^[A-Za-z0-9_-]{1,128}$/.test(target.id);
}
async function call(description:Description,route:string,method:string,args:Record<string,unknown>):Promise<unknown>{
  const header=makeRequest(description.handshake,route,Date.now(),description.context);
  const provenance={product:description.product.id,component:description.product.id+".commands",requestId:header.requestId,revision:catalog.catalogRevision};
  try{
    const response=await invoke<{operation:unknown;value:unknown}>("plugin:commands|"+method,{request:{header,...args}});
    if(!isOperation(response.operation,provenance)||response.operation.outcome.state!=="succeeded")throw new Error("invalid operation");
    return response.value;
  }catch(error){throw new Error(problemMessage(error,provenance));}
}
export async function searchCommands(description:Description,route:string,query:string):Promise<CommandSearch>{
  if(new TextEncoder().encode(query).length>512||/[\x00-\x1f\x7f]/.test(query))throw new Error("검색어 길이와 내용을 확인해 주세요.");
  if(!nativeMode){
    const needle=query.trim().toLocaleLowerCase();
    return {truncated:false,results:catalog.features.filter(feature=>feature.label.toLocaleLowerCase().includes(needle)||feature.command.includes(needle)).map(feature=>({
      id:feature.command,owner:feature.owner,component:feature.component,label:feature.label,revision:"0".repeat(64),
      target:{kind:"route",route:feature.route},requiredContext:"none",context:null,destructive:false,requiresReview:false,
      disabledReason:feature.owner===description.product.id?null:"providerUnavailable",
    }))};
  }
  const value=await call(description,route,"command_search",{query});
  if(!value||typeof value!=="object"||!("results" in value)||!("truncated" in value)
    ||!Array.isArray(value.results)||value.results.length>256||!value.results.every(command)||typeof value.truncated!=="boolean")
    throw new Error("명령 목록의 출처와 형식을 확인하지 못했습니다.");
  return {results:value.results,truncated:value.truncated};
}
export async function previewCommand(description:Description,route:string,item:Command,operationId:string):Promise<Command>{
  if(!nativeMode)return item;
  const value=await call(description,route,"command_preview",{command:{
    operationId,commandId:item.id,revision:item.revision,context:item.context,selectionId:null,
  }});
  if(!command(value)||value.id!==item.id||value.revision!==item.revision)throw new Error("명령의 현재 버전이 바뀌었습니다.");
  return value;
}
