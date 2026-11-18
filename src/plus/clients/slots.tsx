import { lazy } from "react";
import type { DetectedClient } from "@/lib/types";
import { Guard } from "../Guard";

const DirectEntries = lazy(() => import("./DirectEntries"));
const ClientReach = lazy(() => import("./ClientReach"));

/** The one seam ClientDetail.tsx calls: what a client reaches and its Direct entries panel (slice S11). */
export function PlusClientDirect(props: {
  client: DetectedClient;
  onChanged: () => void;
}) {
  return (
    <>
      <Guard name="The reach panel">
        <ClientReach client={props.client} />
      </Guard>
      <Guard name="The direct entries">
        <DirectEntries {...props} />
      </Guard>
    </>
  );
}
