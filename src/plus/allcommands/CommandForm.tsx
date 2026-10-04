import { useId, type Ref } from "react";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { SecretField, type SecretFieldHandle } from "../ui";
import { formFlags, stdinNeed, withheldFlags, type FormValues } from "./model";
import type { CommandFlag, CommandRow } from "../bridge/data";

const UNSET = "__default__";

function secretLabel(row: CommandRow): string {
  if (row.flags.some((flag) => flag.name === "--passphrase-stdin")) return "Passphrase";
  return stdinNeed(row) === "secret" ? "Secret value" : "Input for stdin";
}

/** The form of one command, generated from its operands and flags. Nothing the app must not
 * send is offered: hidden flags, flags that take a secret on the command line and the
 * preview flag are left out, and a secret goes through a write-only field. */
export function CommandForm({
  row,
  values,
  onChange,
  onSecretFilled,
  secret,
  disabled,
}: {
  row: CommandRow;
  values: FormValues;
  onChange: (next: FormValues) => void;
  onSecretFilled: (filled: boolean) => void;
  secret: Ref<SecretFieldHandle>;
  disabled: boolean;
}) {
  const prefix = useId();
  const flags = formFlags(row);
  const need = stdinNeed(row);
  const withheld = need ? withheldFlags(row) : [];
  const setOperand = (name: string, value: string) =>
    onChange({ ...values, operands: { ...values.operands, [name]: value } });
  const setFlag = (name: string, value: string | boolean) =>
    onChange({ ...values, flags: { ...values.flags, [name]: value } });

  if (row.operands.length === 0 && flags.length === 0 && !need) {
    return <p className="text-sm text-muted-foreground">This command takes no input.</p>;
  }
  return (
    <fieldset disabled={disabled} className="flex min-w-0 flex-col gap-4">
      <legend className="sr-only">Input for {row.id}</legend>
      {row.operands.map((operand) => {
        const id = `${prefix}-op-${operand.name}`;
        return (
          <div key={operand.name} className="flex flex-col gap-1.5">
            <div className="flex items-center gap-2">
              <Label htmlFor={id} className="font-mono text-xs">
                {operand.name}
              </Label>
              <span aria-hidden="true" className="text-2xs text-muted-foreground">
                {operand.required ? "required" : "optional"}
              </span>
            </div>
            <Input
              id={id}
              value={values.operands[operand.name] ?? ""}
              onChange={(event) => setOperand(operand.name, event.target.value)}
              aria-required={operand.required || undefined}
              placeholder={operand.variadic ? "Words separated by spaces" : undefined}
              autoComplete="off"
              spellCheck={false}
            />
          </div>
        );
      })}
      {flags.map((flag) => (
        <FlagField
          key={flag.name}
          id={`${prefix}-flag-${flag.name}`}
          flag={flag}
          value={values.flags[flag.name]}
          onChange={(value) => setFlag(flag.name, value)}
        />
      ))}
      {need && (
        <SecretField
          label={secretLabel(row)}
          handle={secret}
          hint={
            need === "secret"
              ? "Sent to the command on stdin. It is never shown, kept or put on a command line."
              : "Optional. Sent to the command on stdin and not kept."
          }
          onFilledChange={onSecretFilled}
          disabled={disabled}
        />
      )}
      {withheld.length > 0 && (
        <p className="text-xs text-muted-foreground">
          Not offered here, because they put a secret on a command line:{" "}
          <span className="font-mono">{withheld.join(", ")}</span>
        </p>
      )}
    </fieldset>
  );
}

function FlagField({
  id,
  flag,
  value,
  onChange,
}: {
  id: string;
  flag: CommandFlag;
  value: string | boolean | undefined;
  onChange: (value: string | boolean) => void;
}) {
  const helpId = `${id}-help`;
  const text = typeof value === "string" ? value : "";
  const label = (
    <div className="flex flex-wrap items-center gap-2">
      <Label htmlFor={id} className="font-mono text-xs">
        {flag.name}
      </Label>
      {flag.required && (
        <span aria-hidden="true" className="text-2xs text-muted-foreground">
          required
        </span>
      )}
      {flag.escalates && (
        <Badge variant="warning" title="Setting this makes the command change things">
          changes things
        </Badge>
      )}
    </div>
  );
  const help = flag.effect ? (
    <p id={helpId} className="text-xs text-muted-foreground">
      {flag.effect}
    </p>
  ) : null;
  const describedBy = flag.effect ? helpId : undefined;

  if (flag.valueType === "bool") {
    return (
      <div className="flex items-start justify-between gap-4">
        <div className="flex min-w-0 flex-col gap-1">
          {label}
          {help}
        </div>
        <Switch
          id={id}
          checked={value === true}
          onCheckedChange={onChange}
          aria-describedby={describedBy}
        />
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-1.5">
      {label}
      {flag.valueType === "choice" ? (
        <Select
          value={text === "" ? UNSET : text}
          onValueChange={(next) => onChange(next === UNSET ? "" : next)}
        >
          <SelectTrigger id={id} aria-describedby={describedBy} className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={UNSET}>Default</SelectItem>
            {(flag.choices ?? []).map((choice) => (
              <SelectItem key={choice} value={choice}>
                {choice}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : flag.repeatable ? (
        <Textarea
          id={id}
          rows={3}
          value={text}
          onChange={(event) => onChange(event.target.value)}
          placeholder="One per line"
          aria-describedby={describedBy}
          aria-required={flag.required || undefined}
          spellCheck={false}
        />
      ) : (
        <Input
          id={id}
          value={text}
          onChange={(event) => onChange(event.target.value)}
          placeholder={
            flag.valueType === "list" || flag.valueType === "paths"
              ? "Separate with commas"
              : undefined
          }
          inputMode={flag.valueType === "integer" ? "numeric" : undefined}
          aria-describedby={describedBy}
          aria-required={flag.required || undefined}
          autoComplete="off"
          spellCheck={false}
        />
      )}
      {help}
    </div>
  );
}
