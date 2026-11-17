import { useEffect, useState } from "react";
import { now } from "./clock";

/** The current time, read again every `every` ms so a line like "in 9 min" stays true. */
export function useNow(every = 10_000): number {
  const [value, setValue] = useState(now);
  useEffect(() => {
    const timer = setInterval(() => setValue(now()), every);
    return () => clearInterval(timer);
  }, [every]);
  return value;
}
