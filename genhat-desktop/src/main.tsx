import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
import App from "./App";
import TourProviderRoot from "./components/TourProviderRoot";
import { TOURS } from "./tours";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <TourProviderRoot tours={TOURS}>
      <App />
    </TourProviderRoot>
  </StrictMode>
);
