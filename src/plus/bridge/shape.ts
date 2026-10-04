/** A tiny runtime shape: one definition gives the TypeScript type of a command's `data` and the
 * check that a golden envelope still has exactly that shape. Objects are strict: a field the
 * shape does not name is drift, because the GUI would not know about it. */
export interface Shape<T = unknown> {
  readonly optional?: boolean;
  validate(value: unknown, path: string, errors: string[]): void;
  /** Never set; carries `T` for inference. */
  readonly __type?: T;
}

export type Infer<S> = S extends Shape<infer T> ? T : never;

type Optional = { optional: true };
type OptionalKeys<P> = {
  [K in keyof P]: P[K] extends Optional ? K : never;
}[keyof P];
type Flatten<T> = { [K in keyof T]: T[K] };
type ObjectOf<P extends Record<string, Shape<unknown>>> = Flatten<
  { [K in Exclude<keyof P, OptionalKeys<P>>]: Infer<P[K]> } & {
    [K in OptionalKeys<P>]?: Infer<P[K]>;
  }
>;

function kindOf(value: unknown): string {
  if (value === null) return "null";
  if (Array.isArray(value)) return "array";
  return typeof value;
}

function primitive<T>(name: string): Shape<T> {
  return {
    validate(value, path, errors) {
      if (kindOf(value) !== name)
        errors.push(`${path}: expected ${name}, got ${kindOf(value)}`);
    },
  };
}

export const str = primitive<string>("string");
export const num = primitive<number>("number");
export const bool = primitive<boolean>("boolean");
export const any: Shape<unknown> = { validate() {} };

/** The goldens replace values that differ per run (`MASKED_KEYS` in `ctl_contract.rs`) with this
 * placeholder; the shape still names the real type, which a live response carries. */
export function masked<T>(inner: Shape<T>): Shape<T> {
  return {
    validate(value, path, errors) {
      if (value !== "<masked>") inner.validate(value, path, errors);
    },
  };
}

export function lit<T extends string>(...values: T[]): Shape<T> {
  return {
    validate(value, path, errors) {
      if (!values.includes(value as T)) {
        errors.push(
          `${path}: expected one of ${values.join(" | ")}, got ${JSON.stringify(value)}`,
        );
      }
    },
  };
}

export function arr<T>(item: Shape<T>): Shape<T[]> {
  return {
    validate(value, path, errors) {
      if (!Array.isArray(value)) {
        errors.push(`${path}: expected array, got ${kindOf(value)}`);
        return;
      }
      value.forEach((entry, i) => item.validate(entry, `${path}[${i}]`, errors));
    },
  };
}

export function nullable<T>(inner: Shape<T>): Shape<T | null> {
  return {
    validate(value, path, errors) {
      if (value !== null) inner.validate(value, path, errors);
    },
  };
}

/** A key that may be absent. */
export function opt<T>(inner: Shape<T>): Shape<T> & Optional {
  return { ...inner, optional: true };
}

export function rec<T>(item: Shape<T>): Shape<Record<string, T>> {
  return {
    validate(value, path, errors) {
      if (kindOf(value) !== "object") {
        errors.push(`${path}: expected object, got ${kindOf(value)}`);
        return;
      }
      for (const [key, entry] of Object.entries(value as object)) {
        item.validate(entry, `${path}.${key}`, errors);
      }
    },
  };
}

export function obj<P extends Record<string, Shape<unknown>>>(
  props: P,
): Shape<ObjectOf<P>> {
  return {
    validate(value, path, errors) {
      if (kindOf(value) !== "object") {
        errors.push(`${path}: expected object, got ${kindOf(value)}`);
        return;
      }
      const record = value as Record<string, unknown>;
      for (const [key, shape] of Object.entries(props)) {
        if (!(key in record)) {
          if (!shape.optional) errors.push(`${path}.${key}: missing`);
          continue;
        }
        shape.validate(record[key], `${path}.${key}`, errors);
      }
      for (const key of Object.keys(record)) {
        if (!(key in props)) errors.push(`${path}.${key}: not in the shape`);
      }
    },
  };
}

export function check(shape: Shape<unknown>, value: unknown, root = "data"): string[] {
  const errors: string[] = [];
  shape.validate(value, root, errors);
  return errors;
}
