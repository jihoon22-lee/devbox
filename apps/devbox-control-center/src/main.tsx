import React from "react";
import ReactDOM from "react-dom/client";
import { ProductShell } from "@devbox/product-shell";
import "./App.css";
import Content from "./Content";
ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ProductShell product="control-center" renderContent={Content} />
  </React.StrictMode>,
);
