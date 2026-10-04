import type { SkillsInstallData } from "../types/skills";
import type { Gate, WriteSpec } from "./hooks";
import { plural } from "./model";

const NO_AUDIT_PHRASE = "install without audit";

/** One install: previewed with the audit. A high-severity finding blocks it, and the only way
 * on is the same install with `--no-audit`, which asks for a typed confirmation of its own. */
export function installSpecOf(
  spec: string,
  target: string,
  skipAudit: boolean,
): WriteSpec {
  const argv = [
    "skills",
    "install",
    spec,
    ...(target.trim() ? ["--path", target.trim()] : []),
    ...(skipAudit ? ["--no-audit"] : []),
  ];
  return skipAudit
    ? {
        command: "skills install",
        title: "Install without the audit",
        argv,
        confirmLabel: "Install without audit",
        phrase: NO_AUDIT_PHRASE,
        typed: true,
      }
    : {
        command: "skills install",
        title: `Install ${spec}`,
        argv,
        confirmLabel: "Install",
        phrase: spec,
        gate: (preview) => installGate(preview, spec, target),
      };
}

export function installGate(preview: unknown, spec: string, target: string): Gate | null {
  const data = preview as Partial<SkillsInstallData> | null;
  if (!data?.blocked) return null;
  const high = data.audit?.high ?? 0;
  return {
    reason: `The audit found ${plural(high, "high-severity finding")} in ${spec}, so the install is blocked. Read the findings; install without the audit only if you trust this source.`,
    override: installSpecOf(spec, target, true),
  };
}
