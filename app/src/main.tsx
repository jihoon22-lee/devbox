import "pretendard/dist/web/variable/pretendardvariable-dynamic-subset.css";
import "./ui/tokens.css";
import "./ui/global.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <p>devbox</p>
  </StrictMode>,
);
