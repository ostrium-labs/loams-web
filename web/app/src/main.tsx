import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { HostProvider } from "@loams/react";
import { domHost } from "@loams/react/dom";
import { App } from "./App";
import "./style.css";
createRoot(document.getElementById("root")!).render(<StrictMode><HostProvider host={domHost}><main><App /></main></HostProvider></StrictMode>);
