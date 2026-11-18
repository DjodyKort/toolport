import type { ReactNode } from "react";

function Key({ children }: { children: string }) {
  return (
    <code className="font-mono text-xs font-normal text-muted-foreground">
      {children}
    </code>
  );
}

export function Help({
  fallback,
  change,
  takes,
}: {
  fallback: string;
  change: ReactNode;
  takes: ReactNode;
}) {
  return (
    <details className="group text-meta">
      <summary className="w-fit cursor-pointer rounded-md text-muted-foreground select-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring focus-visible:outline-none">
        When to change it
      </summary>
      <dl className="mt-1.5 grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1">
        <dt className="text-muted-foreground">Default</dt>
        <dd>{fallback}</dd>
        <dt className="text-muted-foreground">Change it when</dt>
        <dd>{change}</dd>
        <dt className="text-muted-foreground">Takes effect</dt>
        <dd>{takes}</dd>
      </dl>
    </details>
  );
}

export function Setting({
  title,
  registryKey,
  titleId,
  control,
  children,
}: {
  title: string;
  registryKey: string;
  titleId: string;
  control?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section
      aria-labelledby={titleId}
      className="flex flex-col gap-1.5 border-t border-border pt-3 first:border-t-0 first:pt-0"
    >
      <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1.5">
        <h4
          id={titleId}
          className="flex flex-wrap items-baseline gap-x-2 text-sm font-semibold"
        >
          {title} <Key>{registryKey}</Key>
        </h4>
        {control}
      </div>
      {children}
    </section>
  );
}
