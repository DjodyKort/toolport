import { toast } from "sonner";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";

/** One click with Undo: runs the change, says it applies from the next session and offers the
 * opposite change in the toast (10 s). Without `undo` the change is a repair of Toolport's own
 * entry and the toast only reports it. `reload` runs after either. */
export async function changeHooks<R>(
  done: string | ((result: R) => string),
  run: () => Promise<R>,
  undo: (() => Promise<unknown>) | null,
  reload: () => void,
): Promise<boolean> {
  let result: R;
  try {
    result = await run();
  } catch (error) {
    toastError("Could not change the compression hooks", {
      description: errorText(error),
    });
    return false;
  }
  reload();
  toast.success(typeof done === "function" ? done(result) : done, {
    description:
      "Open sessions keep what they loaded; this applies from the next session.",
    duration: 10_000,
    ...(undo && {
      action: {
        label: "Undo",
        onClick: () => {
          undo().then(reload, (error: unknown) =>
            toastError("Could not undo", { description: errorText(error) }),
          );
        },
      },
    }),
  });
  return true;
}

export function copyLine(text: string) {
  void navigator.clipboard?.writeText(text).then(
    () => toast.success("Copied"),
    (error: unknown) => toastError("Could not copy", { description: errorText(error) }),
  );
}
