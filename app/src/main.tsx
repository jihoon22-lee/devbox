import "pretendard/dist/web/variable/pretendardvariable-dynamic-subset.css";
import "./ui/tokens.css";
import "./ui/global.css";
import "./styles/fonts.css";
import "@xterm/xterm/css/xterm.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
