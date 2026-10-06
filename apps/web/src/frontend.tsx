import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./index.css";

const element = document.getElementById("root");
if (!element) throw new Error("The chat root element is missing.");

if (import.meta.hot) {
  const root = (import.meta.hot.data["root"] ??= createRoot(element));
  root.render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
} else {
  createRoot(element).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
