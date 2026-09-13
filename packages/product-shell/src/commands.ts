import {invoke} from "@tauri-apps/api/core";
import {listen} from "@tauri-apps/api/event";
import {makeRequest,nativeMode,isProjectContext,type Description,type ProjectContext} from "./api";
import {isOperation,problemMessage} from "./operation";
import catalog from "../../../apps/products.json";

export interface Command {
  id:string;owner:string;component:string;label:string;revision:string;
  target:{kind:"route";route:string}|{kind:"entity";entity:string;id:string};reviewRoute?:string;
  requiredContext:"none"|"project"|"selection";context:ProjectContext|null;
  destructive:boolean;requiresReview:boolean;disabledReason:string|null;
}
export interface CommandSearch {results:Command[];truncated:boolean;state?:string;partial?:boolean;freshness?:Record<string,{availability:string;indexStale:boolean}>}
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
    ||(row.reviewRoute!==undefined&&(typeof row.reviewRoute!=="string"||!/^[a-z0-9-]{1,96}$/.test(row.reviewRoute)))
    ||(row.destructive&&!row.requiresReview)
    ||(row.disabledReason!==null&&typeof row.disabledReason!=="string")
    ||!row.target||typeof row.target!=="object"||Array.isArray(row.target))return false;
  const target=row.target as Record<string,unknown>;
  return target.kind==="route"?typeof target.route==="string"&&/^[a-z0-9-]{1,96}$/.test(target.route)
    :target.kind==="entity"&&typeof target.entity==="string"&&typeof target.id==="string"&&/^[A-Za-z0-9_-]{1,128}$/.test(target.id);
}
async function call(description:Description,route:string,method:string,args:Record<string,unknown>,signal?:AbortSignal):Promise<unknown>{
  if(signal?.aborted)throw new DOMException("Cancelled","AbortError");
  const header=makeRequest(description.handshake,route,Date.now(),description.context);
  if(["command_preview","command_open","command_trigger_shortcut"].includes(method))header.deadlineMs=Date.now()+29000;
  const cancel=()=>{if(method==="command_source"&&typeof args.product==="string")void call(description,route,"command_cancel",{product:args.product,queryId:header.requestId}).catch(()=>undefined);};
  signal?.addEventListener("abort",cancel,{once:true});
  const provenance={product:description.product.id,component:description.product.id+".commands",requestId:header.requestId,revision:catalog.catalogRevision};
  try{
    const response=await invoke<{operation:unknown;value:unknown}>("plugin:commands|"+method,{request:{header,...args}});
    if(!isOperation(response.operation,provenance)||response.operation.outcome.state!=="succeeded")throw new Error("invalid operation");
    return response.value;
  }catch(error){throw new Error(problemMessage(error,provenance));}
  finally{signal?.removeEventListener("abort",cancel);}
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
export interface CommandReceipt {operationId:string;phase:"awaitingReview"|"opening"|"opened"|"rejected"|"expired"}
function receipt(value:unknown,id:string):CommandReceipt{
  if(!value||typeof value!=="object"||!("operationId" in value)||value.operationId!==id||!("phase" in value)
    ||!["awaitingReview","opening","opened","rejected","expired"].includes(String(value.phase)))throw new Error("명령 전달 결과를 확인하지 못했습니다.");
  return value as CommandReceipt;
}
export async function searchCommandSource(description:Description,route:string,product:string,query:string,generation:number,source="commands",signal?:AbortSignal,mode:"name"|"content"="name"):Promise<CommandSearch>{
  if(!nativeMode)throw new Error("제품 연결을 확인해 주세요.");
  const value=await call(description,route,"command_source",{product,query,generation,source,mode},signal);
  if(!value||typeof value!=="object"||!("generation" in value)||value.generation!==generation
    ||!("source" in value)||value.source!==source||!("owner" in value)||value.owner!==product
    ||!("result" in value)||!value.result||typeof value.result!=="object")throw new Error("검색 출처가 일치하지 않습니다.");
  const result=value.result;
  if(!("results" in result)||!Array.isArray(result.results)||result.results.length>256
    ||!result.results.every(row=>command(row)&&row.owner===product)||!("truncated" in result)||typeof result.truncated!=="boolean")
    throw new Error("검색 결과를 확인할 수 없습니다.");
  const extra=result as Record<string,unknown>;
  const freshness:NonNullable<CommandSearch["freshness"]>={};
  if(extra.freshness&&typeof extra.freshness==="object"&&!Array.isArray(extra.freshness)){
    for(const item of result.results){
      const row=(extra.freshness as Record<string,unknown>)[item.id];
      if(!row||typeof row!=="object"||!("availability" in row)||typeof row.availability!=="string"
        ||!["available","unverified","stale"].includes(row.availability)||!("indexStale" in row)||typeof row.indexStale!=="boolean")throw new Error("검색 결과의 최신 상태를 확인하지 못했습니다.");
      freshness[item.id]={availability:row.availability,indexStale:row.indexStale};
    }
  }
  const state=typeof extra.state==="string"&&["complete","timed_out","unavailable","unsupported","cancelled"].includes(extra.state)?extra.state:undefined;
  return {results:result.results,truncated:result.truncated,state,partial:extra.partial===true,freshness};
}
export async function openCommand(description:Description,route:string,item:Command,operationId:string):Promise<CommandReceipt>{
  if(!nativeMode)throw new Error("브라우저 미리보기에서는 제품을 열 수 없습니다.");
  return receipt(await call(description,route,"command_open",{command:{operationId,commandId:item.id,revision:item.revision,context:item.context,selectionId:null}}),operationId);
}
export async function commandStatus(description:Description,route:string,product:string,operationId:string):Promise<CommandReceipt>{
  if(!nativeMode)throw new Error("제품 연결을 확인해 주세요.");
  return receipt(await call(description,route,"command_status",{product,operationId}),operationId);
}
export interface LauncherPreferences {version:number;favorites:string[];recents:string[]}
export async function launcherPreferences(description:Description,route:string,action:object={kind:"read"}):Promise<LauncherPreferences>{
  if(!nativeMode)return {version:1,favorites:[],recents:[]};
  const value=await call(description,route,"command_preferences",{action});
  if(!value||typeof value!=="object"||!("version" in value)||value.version!==1||!("favorites" in value)||!("recents" in value)
    ||![value.favorites,value.recents].every(ids=>Array.isArray(ids)&&ids.length<=64&&ids.every(id=>typeof id==="string"&&/^[A-Za-z0-9_./:-]{1,256}$/.test(id))))throw new Error("Launcher 설정을 확인하지 못했습니다.");
  return value as LauncherPreferences;
}
export async function launcherShortcut(description:Description,route:string,config:object|null=null):Promise<import("./launcher/types").ShortcutStatus>{
  if(!nativeMode)return {accelerator:"Ctrl+Alt+Space",enabled:false,registration:"unsupported",alternatives:["Ctrl+Alt+L","Ctrl+Alt+J"]};
  const value=await call(description,route,"command_shortcut",{config});
  if(!value||typeof value!=="object"||!("accelerator" in value)||!["Ctrl+Alt+Space","Ctrl+Alt+L","Ctrl+Alt+J"].includes(String(value.accelerator))
    ||!("enabled" in value)||typeof value.enabled!=="boolean"||!("registration" in value)||!["registered","unavailable","unsupported","disabled","pending"].includes(String(value.registration))
    ||!("alternatives" in value)||!Array.isArray(value.alternatives)||value.alternatives.some(key=>!["Ctrl+Alt+Space","Ctrl+Alt+L","Ctrl+Alt+J"].includes(key)))throw new Error("단축키 상태를 확인하지 못했습니다.");
  return value as import("./launcher/types").ShortcutStatus;
}

export async function onShortcut(callback:(event:{command:string;wasFocused:boolean})=>void):Promise<()=>void>{
  if(!nativeMode)return ()=>{};
  return listen<unknown>("suite-shortcut",event=>{
    const value=event.payload;
    if(value&&typeof value==="object"&&"command" in value&&typeof value.command==="string"&&"wasFocused" in value&&typeof value.wasFocused==="boolean"
      &&["control-center.launcher","workspace.summon-terminal","knowledge.quick-capture","workspace.open-current-project"].includes(value.command))callback({command:value.command,wasFocused:value.wasFocused});
  });
}

export async function onConnectionChanged(callback:()=>void):Promise<()=>void>{
  if(!nativeMode)return ()=>{};
  const results=await Promise.allSettled([listen("suite-connected",callback),listen("suite-disconnected",callback)]);
  const stops=results.flatMap(result=>result.status==="fulfilled"?[result.value]:[]);
  return ()=>stops.forEach(stop=>stop());
}

export async function triggerShortcut(description:Description,route:string,command:string,operationId:string):Promise<CommandReceipt>{
  if(!nativeMode)throw new Error("제품 연결을 확인해 주세요.");
  return receipt(await call(description,route,"command_trigger_shortcut",{command,operationId}),operationId);
}

export interface LauncherImport {
  id:string; committed:boolean;
  plan:{preferences:LauncherPreferences; unresolvedFavorites:string[]; unresolvedRecents:string[]; capacityFavorites:string[]; capacityRecents:string[];
    legacyShortcut:{accelerator:string;enabled:boolean}|null; proposedShortcut:import("./launcher/types").ShortcutConfig|null; launcherTerminalConflict:boolean};
}
export async function importLauncher(description:Description,route:string,method:"status"|"preview"|"apply"|"resume",id:string|null=null):Promise<LauncherImport|null>{
  if(!nativeMode)throw new Error("Windows 제품에서 가져오기를 사용할 수 있습니다.");
  const value=await call(description,route,"command_import_launcher",{method,id});
  if(value===null&&method==="status")return null;
  if(!value||typeof value!=="object"||!("id" in value)||typeof value.id!=="string"||!("committed" in value)||typeof value.committed!=="boolean"
    ||!("plan" in value)||!value.plan||typeof value.plan!=="object")throw new Error("가져오기 계획을 확인하지 못했습니다.");
  const plan=value.plan as Record<string,unknown>;
  if(!["unresolvedFavorites","unresolvedRecents","capacityFavorites","capacityRecents"].every(key=>Array.isArray(plan[key])&&(plan[key] as unknown[]).length<=64&&(plan[key] as unknown[]).every(id=>typeof id==="string"&&/^[A-Za-z0-9_./:-]{1,256}$/.test(id))))throw new Error("가져오기 항목을 확인하지 못했습니다.");
  return value as LauncherImport;
}

export async function sendSessionSummary(description:Description,route:string,sourceId:string,operationId:string):Promise<CommandReceipt>{
  if(!nativeMode||description.product.id!=="workspace")throw new Error("Workspace에서 요약을 전달할 수 있습니다.");
  const header=makeRequest(description.handshake,route,Date.now(),description.context);header.deadlineMs=Date.now()+29000;
  const provenance={product:description.product.id,component:"workspace.commands",requestId:header.requestId,revision:catalog.catalogRevision};
  const response=await invoke<{operation:unknown;value:unknown}>("plugin:suite|connection",{request:{header,method:{kind:"sendSessionSummary",sourceId,operationId}}});
  if(!isOperation(response.operation,provenance)||response.operation.outcome.state!=="succeeded")throw new Error("요약 전달 결과를 확인하지 못했습니다.");
  return receipt(response.value,operationId);
}
