import ReviewUpdatesDialog from "./ReviewUpdatesDialog";

/** The plan of moving one server to another version of its package or release. It is the batch
 * plan scoped to that server, so the banner, the bell and the Source row open the same dialog. */
export default function PackageUpdateDialog({
  serverName,
  open,
  onOpenChange,
  onChanged,
}: {
  serverName: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onChanged?: () => void;
}) {
  return (
    <ReviewUpdatesDialog
      servers={[serverName]}
      open={open}
      onOpenChange={onOpenChange}
      onChanged={onChanged}
    />
  );
}
