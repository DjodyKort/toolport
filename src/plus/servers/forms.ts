import type { ProfileData, ServerInfo } from "./model";
import type { EditChange, ServerFields } from "./plans";

export interface ServerForm {
  name: string;
  kind: "command" | "url";
  command: string;
  /** One argument per line. */
  args: string;
  url: string;
  transport: string;
  cwd: string;
  declareClientCapabilities: boolean;
  forwardInstructions: boolean;
}

export const emptyForm = (): ServerForm => ({
  name: "",
  kind: "command",
  command: "",
  args: "",
  url: "",
  transport: "",
  cwd: "",
  declareClientCapabilities: false,
  forwardInstructions: false,
});

export function formOf(info: ServerInfo): ServerForm {
  return {
    name: info.name,
    kind: info.url ? "url" : "command",
    command: info.command ?? "",
    args: info.args.join("\n"),
    url: info.url ?? "",
    transport: info.transport,
    cwd: info.cwd ?? "",
    declareClientCapabilities: info.declareClientCapabilities,
    forwardInstructions: info.forwardInstructions,
  };
}

export const argList = (text: string): string[] =>
  text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);

export function fieldsOf(form: ServerForm): ServerFields {
  return {
    name: form.name.trim(),
    kind: form.kind,
    command: form.command.trim(),
    args: argList(form.args),
    url: form.url.trim(),
    transport: form.transport,
    cwd: form.cwd.trim(),
  };
}

export function newProblems(form: ServerForm, existing: string[]): string[] {
  const fields = fieldsOf(form);
  const problems: string[] = [];
  if (!fields.name) problems.push("Give the server a name");
  else if (existing.some((name) => name.toLowerCase() === fields.name.toLowerCase()))
    problems.push(`A server called ${fields.name} already exists`);
  if (fields.kind === "command" && !fields.command)
    problems.push("Give the command that starts it");
  if (fields.kind === "url" && !/^https?:\/\/\S+$/i.test(fields.url))
    problems.push("Give the address, starting with http:// or https://");
  return problems;
}

export function newServerArgv(fields: ServerFields): string[] {
  const argv = ["server", "new", fields.name];
  if (fields.kind === "url") {
    argv.push("--url", fields.url);
    if (fields.transport) argv.push("--transport", fields.transport);
  } else {
    argv.push("--command", fields.command);
    for (const arg of fields.args) argv.push("--arg", arg);
  }
  if (fields.cwd) argv.push("--cwd", fields.cwd);
  return argv;
}

export interface EditPlan {
  argv: string[];
  undo: string[] | null;
  changes: EditChange[];
  problems: string[];
}

const onOff = (value: boolean) => (value ? "on" : "off");

/** What an edit changes, as the flags `server edit` takes. `--arg` replaces the whole list and
 * nothing clears a value, so those two cases are reported instead of sent. */
export function editPlan(info: ServerInfo, form: ServerForm): EditPlan {
  const fields = fieldsOf(form);
  const argv = ["server", "edit", info.id];
  const undo = ["server", "edit", info.id];
  const changes: EditChange[] = [];
  const problems: string[] = [];
  let reversible = true;

  const text = (
    field: string,
    flag: string,
    before: string,
    after: string,
    clearable = false,
  ) => {
    if (before === after) return;
    if (!after && !clearable) {
      problems.push(`${field} cannot be emptied with the CLI`);
      return;
    }
    argv.push(flag, after);
    changes.push({ field, before, after });
    if (before) undo.push(flag, before);
    else reversible = false;
  };

  if (!fields.name) problems.push("Give the server a name");
  text("name", "--name", info.name, fields.name);
  if (info.command !== null) {
    if (!fields.command) problems.push("Give the command that starts it");
    else text("command", "--command", info.command, fields.command);
    const before = info.args;
    if (before.join("\n") !== fields.args.join("\n")) {
      if (fields.args.length === 0) {
        problems.push("The arguments cannot be emptied with the CLI");
      } else {
        for (const arg of fields.args) argv.push("--arg", arg);
        changes.push({
          field: "arguments",
          before: before.join("\n"),
          after: fields.args.join("\n"),
        });
        if (before.length > 0) for (const arg of before) undo.push("--arg", arg);
        else reversible = false;
      }
    }
  }
  if (info.url !== null) {
    if (!/^https?:\/\/\S+$/i.test(fields.url))
      problems.push("Give the address, starting with http:// or https://");
    else text("url", "--url", info.url, fields.url);
    text("transport", "--transport", info.transport, fields.transport);
  }
  text("working folder", "--cwd", info.cwd ?? "", fields.cwd);
  const flags: Array<[string, string, boolean, boolean]> = [
    [
      "declare client capabilities",
      "--declare-client-capabilities",
      info.declareClientCapabilities,
      form.declareClientCapabilities,
    ],
    [
      "forward instructions",
      "--forward-instructions",
      info.forwardInstructions,
      form.forwardInstructions,
    ],
  ];
  for (const [field, flag, before, after] of flags) {
    if (before === after) continue;
    argv.push(flag, onOff(after));
    undo.push(flag, onOff(before));
    changes.push({ field, before: onOff(before), after: onOff(after) });
  }
  return {
    argv,
    undo: reversible && changes.length > 0 ? undo : null,
    changes,
    problems,
  };
}

export interface ProfileEdit {
  name: string;
  /** Server ids that end up in the profile. */
  servers: string[];
}

/** Flags of `profile edit` for a change: it takes one server option at a time, so adds and
 * removes together become the whole set. */
export function profileEditArgv(profile: ProfileData, edit: ProfileEdit): string[] {
  const before = profile.servers.map((server) => server.id);
  const added = edit.servers.filter((id) => !before.includes(id));
  const removed = before.filter((id) => !edit.servers.includes(id));
  const argv = ["profile", "edit", profile.id];
  if (edit.name.trim() !== profile.name) argv.push("--name", edit.name.trim());
  if (added.length > 0 && removed.length > 0)
    argv.push("--set-servers", edit.servers.join(","));
  else if (added.length > 0) argv.push("--add-server", added.join(","));
  else if (removed.length > 0) argv.push("--remove-server", removed.join(","));
  return argv;
}
