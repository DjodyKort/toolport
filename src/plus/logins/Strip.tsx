import { Stat, WhenText } from "./atoms";
import type { Summary } from "./model";

/** The three facts at the top of every tab: how many logins work, which ones do not, and when
 * the last probe ran. */
export function Strip({
  summary,
  lastProbe = true,
}: {
  summary: Summary;
  /** The statusline has no probe time, so the Integrations tab leaves the tile out. */
  lastProbe?: boolean;
}) {
  const need = summary.needSignIn;
  return (
    <div
      role="group"
      aria-label="Login summary"
      className="grid gap-3 [grid-template-columns:repeat(auto-fit,minmax(190px,1fr))]"
    >
      <Stat
        label="Signed in"
        tone={summary.signedIn === summary.total ? "ok" : undefined}
        value={`${summary.signedIn} of ${summary.total}`}
        note={summary.other > 0 ? `${summary.other} more need a look` : undefined}
      />
      <Stat
        label="Needs a sign-in"
        tone={need > 0 ? "warn" : "ok"}
        value={need}
        note={need > 0 ? summary.needNames.join(", ") : "Nothing to do"}
      />
      {lastProbe && (
        <Stat
          label="Last probe"
          value={<WhenText seconds={summary.lastProbe} />}
          note="Probes run on their own"
        />
      )}
    </div>
  );
}
