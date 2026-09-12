import React, {lazy,Suspense} from "react";
import ReactDOM from "react-dom/client";
import {ProductShell,type ShellContentProps} from "@devbox/product-shell";
import "./App.css";
const Commands=lazy(()=>import("./Commands"));
const RouteView=lazy(()=>import("@devbox/product-shell/route-view"));
function Content(props:ShellContentProps) {
  const feature=props.description.features.find(feature=>feature.route===props.route);
  return <Suspense fallback={<p role="status">화면을 불러오고 있습니다…</p>}>{
    props.route==="products"?<Commands {...props}/>:feature?<RouteView description={props.description} feature={feature}/>:null
  }</Suspense>;
}
ReactDOM.createRoot(document.getElementById("root")!).render(<React.StrictMode><ProductShell product="control-center" renderContent={Content}/></React.StrictMode>);
