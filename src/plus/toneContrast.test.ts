import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const css = readFileSync(join(__dirname, "../index.css"), "utf8");
const badge = readFileSync(join(__dirname, "../components/ui/badge.tsx"), "utf8");

type Rgb = [number, number, number];

function block(selector: string): string {
  const start = css.indexOf(`${selector} {`);
  expect(start, `${selector} block`).toBeGreaterThan(-1);
  return css.slice(start, css.indexOf("\n}", start));
}

function token(scope: string, name: string): [number, number, number] {
  const match = scope.match(
    new RegExp(`--${name}: oklch\\(([\\d.]+) ([\\d.]+) ([\\d.]+)\\)`),
  );
  expect(match, `--${name}`).not.toBeNull();
  return [Number(match![1]), Number(match![2]), Number(match![3])];
}

function linearOf([lightness, chroma, hue]: [number, number, number]): Rgb {
  const a = chroma * Math.cos((hue * Math.PI) / 180);
  const b = chroma * Math.sin((hue * Math.PI) / 180);
  const l = (lightness + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (lightness - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (lightness - 0.0894841775 * a - 1.291485548 * b) ** 3;
  const rgb: Rgb = [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ];
  return rgb.map((x) => Math.min(1, Math.max(0, x))) as Rgb;
}

const gamma = (x: number) =>
  x <= 0.0031308 ? 12.92 * x : 1.055 * x ** (1 / 2.4) - 0.055;
const linear = (x: number) => (x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4);
const luminance = ([r, g, b]: Rgb) => 0.2126 * r + 0.7152 * g + 0.0722 * b;

function ratio(a: Rgb, b: Rgb): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** The tone painted at `alpha` over `base`, mixed in gamma space like the browser does. */
function tint(tone: Rgb, alpha: number, base: Rgb): Rgb {
  return tone.map((x, i) =>
    linear(alpha * gamma(x) + (1 - alpha) * gamma(base[i])),
  ) as Rgb;
}

function alphaOf(tone: string, dark: boolean): number {
  const prefix = dark ? `dark:bg-${tone}/` : `(?<![:\\w])bg-${tone}/`;
  const match = badge.match(new RegExp(`${prefix}(\\d+)`));
  expect(match, `badge tint of ${tone}`).not.toBeNull();
  return Number(match![1]) / 100;
}

const TONES = ["success", "warning", "info", "owned"];
const light = block(":root");
const dark = block(".dark");

describe("badge tones reach 4.5:1 as text", () => {
  it.each([...TONES, "destructive"])(
    "light %s on its own tint and on the page",
    (tone) => {
      const text = linearOf(token(light, tone));
      const page = linearOf(token(light, "background"));
      expect(
        ratio(text, tint(text, alphaOf(tone, false), page)),
        `${tone} badge`,
      ).toBeGreaterThanOrEqual(4.5);
      expect(ratio(text, page), `${tone} text on the page`).toBeGreaterThanOrEqual(4.5);
    },
  );

  it.each(TONES)("dark %s on its own tint", (tone) => {
    const text = linearOf(token(dark, tone));
    const card = linearOf(token(dark, "card"));
    expect(
      ratio(text, tint(text, alphaOf(tone, true), card)),
      `${tone} badge`,
    ).toBeGreaterThanOrEqual(4.5);
  });
});
