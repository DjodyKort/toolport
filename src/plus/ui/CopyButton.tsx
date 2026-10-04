import { useState } from "react";
import { Check, Copy } from "lucide-react";
import { toastError } from "@/lib/toast";
import { Button } from "@/components/ui/button";

/** Copies fixed, non-secret text such as a command line or an address. */
export function CopyButton({
  text,
  label = "Copy",
  size = "sm",
}: {
  text: string;
  label?: string;
  size?: "xs" | "sm";
}) {
  const [copied, setCopied] = useState(false);
  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      toastError("Couldn't copy to the clipboard");
    }
  }
  return (
    <Button type="button" size={size} variant="outline" onClick={() => void copy()}>
      {copied ? <Check /> : <Copy />}
      {copied ? "Copied" : label}
    </Button>
  );
}
