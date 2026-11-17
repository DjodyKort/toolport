import { toast } from "sonner";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import type { WriteData } from "./data";

/** One click with Undo: runs the change, says what happened and that it applies from the next
 * session, and offers the opposite change in the toast (10 s). `reload` runs after either. */
export async function changeWithUndo(
  done: string,
  run: () => Promise<WriteData>,
  undo: () => Promise<WriteData>,
  reload: () => void,
): Promise<boolean> {
  try {
    await run();
  } catch (error) {
    toastError(`Could not change this folder`, { description: errorText(error) });
    return false;
  }
  reload();
  toast.success(done, {
    description:
      "Open sessions keep what they loaded; this applies from the next session.",
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
