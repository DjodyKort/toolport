import type { TeamPushPreview } from "@/lib/api";

/** Only the backend's display allowlist, never a registry entry or resolved credentials. */
export function TeamSharePreview({ preview }: { preview: TeamPushPreview }) {
  return (
    <div className="grid max-h-[60vh] gap-3 overflow-y-auto text-left">
      <p>
        Selected definitions are added or updated. Other team servers, instructions and
        policies stay unchanged. Your personal servers remain saved.
      </p>
      <p>
        Credential values are never uploaded. Each member uses their own credentials
        locally.
      </p>
      {(["Added", "Changed", "Removed"] as const).map((label) => {
        const names = preview[label.toLowerCase() as "added" | "changed" | "removed"];
        return (
          <section key={label}>
            <div className="font-medium text-foreground">
              {label} ({names.length})
            </div>
            {names.length === 0 ? (
              <p className="mt-1">None</p>
            ) : label === "Removed" ? (
              <ul className="mt-1 list-disc pl-5">
                {names.map((name, i) => (
                  <li key={i} className="break-all">
                    {name}
                  </li>
                ))}
              </ul>
            ) : (
              <div className="mt-1 grid gap-2">
                {preview.definitions
                  .filter((d) => d.change === label)
                  .map((d) => (
                    <details
                      key={d.id}
                      open={preview.definitions.length === 1}
                      className="min-w-0 rounded-md border p-2"
                    >
                      <summary className="cursor-pointer break-all text-foreground">
                        {d.name}{" "}
                        <span className="text-muted-foreground">· {d.transport}</span>
                      </summary>
                      <dl className="mt-2 grid gap-2">
                        {d.fields.map((field) => (
                          <div key={field.label}>
                            <dt className="text-xs font-medium">{field.label}</dt>
                            <dd className="mt-0.5 whitespace-pre-wrap break-all font-mono text-xs text-foreground">
                              {field.value}
                            </dd>
                          </div>
                        ))}
                      </dl>
                    </details>
                  ))}
              </div>
            )}
          </section>
        );
      })}
      <p>
        If the team or your local servers change before saving, Toolport stops and asks
        you to review again.
      </p>
    </div>
  );
}
