import React, { lazy, Suspense } from "react";
import ReactDOM from "react-dom/client";
import Workspace from "./Workspace";
import "./App.css";
const TerminalCompanion=lazy(()=>import("./TerminalCompanion"));
const terminal=new URLSearchParams(window.location.search).get("surface")==="terminal";
ReactDOM.createRoot(document.getElementById("root")!).render(<React.StrictMode>{terminal?<Suspense fallback={<p role="status">터미널을 불러오고 있습니다…</p>}><TerminalCompanion/></Suspense>:<Workspace />}</React.StrictMode>);
