import {createContext,useContext} from "react";
import type {ProjectContext} from "./api";
/** A renderer hint for an existing owner review UI, never an access grant. */
export interface IncomingReview {
 operationId:string;revision:string;commandRevision:string;label:string;route:string;
 target:{kind:"route";route:string}|{kind:"entity";entity:string;id:string};
 context:ProjectContext|null;
}
export const IncomingReviewContext=createContext<{review:IncomingReview|null;clear:()=>void}>({review:null,clear:()=>{}});
export const useIncomingReview=()=>useContext(IncomingReviewContext);
