import {useEffect,useMemo,useRef} from "react";
import Launcher from "@devbox/product-shell/launcher";
import type {LauncherAdapter} from "@devbox/product-shell/launcher-adapter";
import type {SearchResponse,SearchResult,SourceDiagnostic} from "@devbox/product-shell/launcher-types";
import type {ShellContentProps} from "@devbox/product-shell";
import {searchCommands,searchCommandSource,previewCommand,openCommand,launcherPreferences,launcherShortcut,type Command,type CommandSearch,type LauncherPreferences} from "@devbox/product-shell/commands";
const providers=["workspace","api-studio","knowledge"];
export default function HostedLauncher({description,route,navigate,close}:ShellContentProps&{close:()=>void}){
  const dialog=useRef<HTMLDialogElement>(null);
  useEffect(()=>{const previous=document.activeElement;dialog.current?.showModal();return()=>{if(previous instanceof HTMLElement&&previous.isConnected)previous.focus();};},[]);
  const adapter=useMemo<LauncherAdapter>(()=>{
    let generation=0,current:Command[]=[];let controller:AbortController|undefined;
    let preferences:LauncherPreferences={version:1,favorites:[],recents:[]};
    const listeners=new Set<(response:SearchResponse)=>void>();
    const result=(sources:SourceDiagnostic[]):SearchResponse=>({sources,results:current.map(item=>({
      id:item.id,revision:item.revision,label:item.label,detail:item.owner,source:item.owner,targetApp:item.owner,targetKind:item.target.kind,
      stale:false,explicitPreview:false,favorite:preferences.favorites.includes(item.id),recent:preferences.recents.includes(item.id),
      ...(item.disabledReason?{disabledReason:"제품 연결 확인 필요"}:{}),
    })).sort((a,b)=>Number(b.favorite)-Number(a.favorite)||Number(b.recent)-Number(a.recent))});
    const command=(item:SearchResult)=>{const found=current.find(row=>row.id===item.id&&row.revision===item.revision);if(!found||found.disabledReason)throw new Error("stale command");return found;};
    const request=(item:Command,operationId:string)=>({operationId,commandId:item.id,revision:item.revision,context:item.context,selectionId:null});
    return {
      async search(query){
        controller?.abort();controller=new AbortController();const signal=controller.signal;
        const ticket=++generation;
        const base=await searchCommands(description,route,query);
        try{preferences=await launcherPreferences(description,route);}catch{/* Search remains available; writes still reject corrupt preferences. */}
        if(ticket!==generation)return {results:[],sources:[]};
        current=base.results;
        const sources:SourceDiagnostic[]=providers.map(producer=>({producer,view:"commands",status:"missing"}));
        for(const product of providers){
          void searchCommandSource(description,route,product,query,ticket,"commands",signal).then((remote:CommandSearch)=>{
            if(ticket!==generation)return;
            current=[...current.filter(item=>item.owner!==product),...remote.results].slice(0,256);
            sources.find(source=>source.producer===product)!.status="fresh";
            const next=result([...sources]);for(const listener of listeners)listener(next);
          }).catch(()=>{/* Per-source missing state remains separate from other results. */});
        }
        return result(sources);
      },
      async launchResult(item){
        const operationId=crypto.randomUUID();const selected=await previewCommand(description,route,command(item),operationId);
        if(selected.owner===description.product.id&&selected.target.kind==="route"){
          navigate(selected.target.route);
          await launcherPreferences(description,selected.target.route,{kind:"visit",command:request(selected,operationId)}).catch(()=>undefined);
          return {status:"launched",appId:selected.owner};
        }
        const receipt=await openCommand(description,route,selected,operationId);
        return {status:receipt.phase==="opened"?"launched":"reviewPending",appId:selected.owner};
      },
      async setFavorite(item,favorite){const selected=favorite?command(item):current.find(row=>row.id===item.id);if(!selected)throw new Error("unknown command");preferences=await launcherPreferences(description,route,{kind:"favorite",command:request(selected,crypto.randomUUID()),favorite});},
      async clearRecents(){preferences=await launcherPreferences(description,route,{kind:"clearRecents"});},
      getShortcut:()=>launcherShortcut(description,route),
      setShortcut:config=>launcherShortcut(description,route,config),
      hide:async()=>{generation++;controller?.abort();close();},
      subscribe:listener=>{listeners.add(listener);return()=>{listeners.delete(listener);generation++;controller?.abort();};},
      // No clipboard/text action is offered by this command-only source. The
      // separate source-owned Artifact adapter will supply explicit selections.
      previewTextAction:async()=>{throw new Error("text action unavailable");},
      performTextAction:async()=>{throw new Error("text action unavailable");},
      readCurrentText:async()=>{throw new Error("text action unavailable");},
    };
  },[description,route,navigate,close]);
  return <dialog ref={dialog} className="hosted-launcher" onCancel={event=>{event.preventDefault();close();}}><Launcher adapter={adapter} embedded description="네 제품의 명령을 찾고 해당 제품에서 작업을 확인합니다."/></dialog>;
}
