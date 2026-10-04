import { useState } from "react";
import { PackageOpen, Package } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { commandLine } from "../allcommands/model";
import { CopyButton } from "../ui";
import { PathField } from "./fields";
import type { WriteControl } from "./hooks";
import { Section } from "./parts";

function Actions({
  argv,
  disabled,
  label,
  icon,
  onRun,
}: {
  argv: string[];
  disabled: boolean;
  label: string;
  icon: React.ReactNode;
  onRun: () => void;
}) {
  const line = commandLine(argv);
  return (
    <div className="flex flex-wrap items-center gap-2">
      <Button type="button" disabled={disabled} onClick={onRun}>
        {icon} {label}
      </Button>
      <CopyButton text={line} label="Copy command" />
      <code className="font-mono text-xs break-all text-muted-foreground">{line}</code>
    </div>
  );
}

function Bundle({ write }: { write: WriteControl }) {
  const [skills, setSkills] = useState("");
  const [output, setOutput] = useState("");
  const names = skills
    .split(",")
    .map((name) => name.trim())
    .filter(Boolean);
  const argv = [
    "skills",
    "bundle",
    ...(names.length > 0 ? ["--skills", names.join(",")] : []),
    ...(output.trim() ? ["--output", output.trim()] : []),
  ];
  return (
    <div className="flex flex-col gap-3">
      <label className="flex flex-col gap-1.5 text-sm">
        Skills
        <Input
          value={skills}
          onChange={(event) => setSkills(event.target.value)}
          placeholder="api-review, deploy-helper"
          autoComplete="off"
          spellCheck={false}
        />
        <span className="text-xs text-muted-foreground">
          Comma-separated names. Empty packs every skill and rule of your library.
        </span>
      </label>
      <PathField
        label="Zip file"
        kind="save"
        value={output}
        onChange={setOutput}
        placeholder="Empty: <repository>-bundle.zip in the repository"
      />
      <Actions
        argv={argv}
        disabled={write.busy}
        label="Preview bundle"
        icon={<Package />}
        onRun={() =>
          write.begin({
            command: "skills bundle",
            title: "Pack skills into a zip",
            argv,
            confirmLabel: "Pack",
            phrase: "pack",
          })
        }
      />
    </div>
  );
}

function Unbundle({ write }: { write: WriteControl }) {
  const [zip, setZip] = useState("");
  const [target, setTarget] = useState("");
  const [touched, setTouched] = useState(false);
  const bundle = zip.trim();
  const argv = [
    "skills",
    "unbundle",
    bundle || "<bundle.zip>",
    ...(target.trim() ? ["--path", target.trim()] : []),
  ];
  return (
    <div className="flex flex-col gap-3">
      <PathField label="Bundle zip" kind="file" value={zip} onChange={setZip} />
      {touched && !bundle && (
        <p role="status" className="text-xs text-destructive">
          Give the zip file to extract
        </p>
      )}
      <PathField
        label="Extract into"
        kind="folder"
        value={target}
        onChange={setTarget}
        placeholder="Empty: your skills repository"
        hint="Files that already exist are listed in the preview before anything is overwritten."
      />
      <Actions
        argv={argv}
        disabled={write.busy}
        label="Preview unbundle"
        icon={<PackageOpen />}
        onRun={() => {
          setTouched(true);
          if (!bundle) return;
          write.begin({
            command: "skills unbundle",
            title: "Extract a skills bundle",
            argv,
            confirmLabel: "Extract",
            phrase: "extract",
          });
        }}
      />
    </div>
  );
}

/** Pack skills into a portable zip, or extract one. The CLI takes paths, so these are text
 * fields (with the native picker as a convenience) and a copy of the exact command. */
export function BundlesPanel({ write }: { write: WriteControl }) {
  return (
    <div className="flex flex-col gap-6">
      <Section title="Pack skills into a zip">
        <Bundle write={write} />
      </Section>
      <Section title="Extract a bundle">
        <Unbundle write={write} />
      </Section>
    </div>
  );
}
