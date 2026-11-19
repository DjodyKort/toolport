import { toast } from "sonner";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import { UNDO_MS } from "./bodyEdit";
import { linkTap, unlinkTap } from "./commands";
import type { TapLinkData, TapRow } from "./data";
import { Mono, PlanSteps, WriteDialog } from "./dialogs";

/** The plan of `skills tap link` or `unlink`, then the write. Undo on the message runs the
 * opposite command; Claude Code reads plugin settings when a session starts, so running
 * sessions keep the old state until they restart (the plan says so). */
export default function TapLinkDialog({
  tap,
  link,
  onClose,
  onDone,
}: {
  tap: TapRow;
  link: boolean;
  onClose: () => void;
  onDone: () => void;
}) {
  const name = tap.name;
  const verb = link ? linkTap : unlinkTap;
  const market = tap.marketplace?.name ?? name;

  async function confirm() {
    const done = await verb(name, false);
    if (done.changes.length === 0) return done;
    toast.message(
      link ? `${name} is linked to Claude Code` : `${name} is unlinked from Claude Code`,
      {
        description:
          "Sessions that are already running keep the old state until they restart.",
        duration: UNDO_MS,
        action: {
          label: "Undo",
          onClick: () =>
            void (link ? unlinkTap : linkTap)(name, false)
              .catch((error: unknown) =>
                toastError(`Could not undo the change to ${name}`, {
                  description: errorText(error),
                }),
              )
              .finally(onDone),
        },
      },
    );
    return done;
  }

  return (
    <WriteDialog<TapLinkData>
      title={link ? `Link ${name} to Claude Code` : `Unlink ${name} from Claude Code`}
      description={
        link
          ? `Declares the clone of ${name} as the plugin marketplace ${market} in Claude Code's user settings and switches its plugins on. The tap stays where it is, so edits in it apply in the next session.`
          : `Takes the marketplace ${market} and its plugins out of Claude Code's user settings. The tap and its clone stay.`
      }
      confirmLabel={link ? "Link" : "Unlink"}
      destructive={!link}
      preview={() => verb(name, true)}
      blocked={(data) =>
        data.changes.length === 0
          ? link
            ? "Already linked: there is nothing to write."
            : "Not linked: there is nothing to remove."
          : null
      }
      render={(data) => (
        <div className="flex flex-col gap-3">
          <PlanSteps plan={data.plan} />
          {data.changes.length > 0 && (
            <ul className="flex flex-col gap-0.5 text-meta" aria-label="Settings keys">
              {data.changes.map((change) => (
                <li key={change.key}>
                  {change.action} <Mono>{change.key}</Mono>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
      confirm={confirm}
      onClose={onClose}
      onDone={onDone}
    />
  );
}
