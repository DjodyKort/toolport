import { toast } from "sonner";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";

/** Runs one change and offers its opposite in the toast for 10 s; `reload` runs after either. */
export async function withUndo(
  done: string,
  run: () => Promise<unknown>,
  undo: () => Promise<unknown>,
  reload: () => void,
  description = "Open sessions keep what they loaded; this applies from the next session.",
): Promise<boolean> {
  try {
    await run();
  } catch (error) {
    toastError("Could not change memory and history", { description: errorText(error) });
    return false;
  }
  reload();
  toast.success(done, {
    description,
    duration: 10_000,
    action: {
      label: "Undo",
      onClick: () => {
        undo().then(reload, (error: unknown) =>
          toastError("Could not undo", { description: errorText(error) }),
        );
      },
    },
  });
  return true;
}
