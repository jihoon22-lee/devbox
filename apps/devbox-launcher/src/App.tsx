import {getCurrentWindow} from "@tauri-apps/api/window";
import Launcher from "@devbox/product-shell/launcher";
import * as api from "./api";
import {isTauri} from "./lib/isTauri";
import "./App.css";
const adapter={...api,hide:async()=>{if(isTauri())await getCurrentWindow().hide();}};
export default function App(){return <Launcher adapter={adapter}/>;}
