import { Badge } from "@/components/ui/badge";
import { EmptyState } from "@/components/ui/empty-state";
import type { DoctorData, StatusData } from "../bridge/data";
import { CHECK_BADGE, CHECK_LABELS, checkLabel, plural } from "./model";

function Fact({ term, children }: { term: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-wrap gap-x-3 gap-y-0.5 px-4 py-2">
      <dt className="w-40 shrink-0 text-meta font-medium">{term}</dt>
      <dd className="min-w-0 flex-1 text-meta break-all text-muted-foreground">
        {children}
      </dd>
    </div>
  );
}

/** The doctor's checks. When they could not run, the facts `status` reports stand in for them. */
export function Installation({
  doctor,
  status,
  doctorError,
  statusError,
}: {
  doctor: DoctorData | null;
  status: StatusData | null;
  doctorError: string | null;
  statusError: string | null;
}) {
  return (
    <div className="toolport-list">
      {doctor ? (
        <ul>
          {doctor.checks.map((check) => {
            const badge = CHECK_BADGE[check.status];
            return (
              <li
                key={check.name}
                className="flex flex-wrap items-center gap-x-3 gap-y-0.5 border-b border-border px-4 py-2.5"
              >
                <span className="w-full text-sm font-medium sm:w-40 sm:shrink-0">
                  {checkLabel(check.name, CHECK_LABELS)}
                </span>
                <Badge variant={badge.variant}>{badge.text}</Badge>
                <span className="min-w-48 flex-1 font-mono text-xs break-all text-muted-foreground">
                  {check.detail}
                </span>
              </li>
            );
          })}
        </ul>
      ) : (
        <EmptyState
          framed={false}
          title="The installation checks did not run"
          description={doctorError ?? "No result yet."}
        />
      )}
      {status && doctor && (
        <p className="bg-muted/30 px-4 py-2 text-meta text-muted-foreground">
          Toolport {status.version}
        </p>
      )}
      {status && !doctor && (
        <dl className="border-t border-border bg-muted/30">
          <Fact term="Version">Toolport {status.version}</Fact>
          <Fact term="Data folder">{status.dataDir}</Fact>
          <Fact term="Registry">
            {plural(status.serverCount, "server")},{" "}
            {plural(status.accessSetCount, "access set")}
          </Fact>
        </dl>
      )}
      {!status && statusError && (
        <p className="border-t border-border px-4 py-3 text-meta text-muted-foreground">
          Status unavailable: {statusError}
        </p>
      )}
    </div>
  );
}
