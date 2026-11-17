import { Wrench } from "lucide-react";
import { StatusCard } from "@/components/ui/status-card";
import { Group, RowShell } from "./StackRows";

/** The compression slice (S7) is not merged: this tab shows what will live here and reads
 * nothing, so no number on it can pretend to be a measurement. */
export default function HooksCompression() {
  return (
    <div className="flex flex-col gap-5">
      <StatusCard
        variant="info"
        icon={<Wrench />}
        title="Hooks and compression land with slice S7"
        detail="Nothing on this tab works yet and it shows no data. The hooks and compression commands are not part of this build."
      />
      <Group title="Planned" hint="from the approved design">
        <RowShell
          title="Inventory of hooks"
          subtitle="What runs around a tool call, who owns each entry, and the real action per row."
        />
        <RowShell
          title="Compression hooks"
          subtitle="Adopt the rtk and read-once entries with a backup and Undo, verify them, and count rewrites, bytes and latency at the hook."
        />
        <RowShell
          title="Compression measurements"
          subtitle="Four honest questions with their population and limits, as a drill-down."
        />
      </Group>
    </div>
  );
}
