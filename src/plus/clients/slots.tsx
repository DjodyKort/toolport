import { lazy } from "react";
import type { DetectedClient } from "@/lib/types";
import { Guard } from "../Guard";

const DirectEntries = lazy(() => import("./DirectEntries"));

/** The one seam ClientDetail.tsx calls: the Direct entries panel of a client (slice S11). */
export function PlusClientDirect(props: {
  client: DetectedClient;
  onChanged: () => void;
}) {
  return (
    <Guard name="The direct entries">
      <DirectEntries {...props} />
    </Guard>
  );
}
