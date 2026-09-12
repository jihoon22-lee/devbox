import type {SearchResponse,SearchResult,LaunchResponse,ShortcutConfig,ShortcutStatus} from "./types";
export interface LauncherAdapter {
 search(query:string):Promise<SearchResponse>;
 launchResult(result:SearchResult,allowStale?:boolean):Promise<LaunchResponse>;
 previewTextAction(result:SearchResult):Promise<{actionId:string;kind:string;maxBytes:number}>;
 performTextAction(result:SearchResult,text:string):Promise<LaunchResponse>;
 setFavorite(result:SearchResult,favorite:boolean):Promise<void>;
 clearRecents():Promise<void>;
 readCurrentText():Promise<string>;
 getShortcut():Promise<ShortcutStatus>;
 setShortcut(config:ShortcutConfig):Promise<ShortcutStatus>;
 hide():Promise<void>;
 subscribe?(listener:(response:SearchResponse)=>void):()=>void;
}
