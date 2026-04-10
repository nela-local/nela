import React from "react";
import { TourProvider } from "../hooks/useTour";
import TourOverlay from "./TourOverlay";
import type { TourDefinition } from "../hooks/useTour";

export default function TourProviderRoot({
  tours,
  children,
}: {
  tours: TourDefinition[];
  children: React.ReactNode;
}) {
  return (
    <TourProvider tours={tours}>
      {children}
      <TourOverlay />
    </TourProvider>
  );
}
