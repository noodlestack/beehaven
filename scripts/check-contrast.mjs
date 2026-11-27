import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const tokensPath = path.resolve(__dirname, "../design/tokens.json");
const tokens = JSON.parse(fs.readFileSync(tokensPath, "utf8"));

function hexToRgb(hex) {
  const clean = hex.replace("#", "");
  return [
    parseInt(clean.slice(0, 2), 16),
    parseInt(clean.slice(2, 4), 16),
    parseInt(clean.slice(4, 6), 16),
  ];
}

function relativeLuminance([r, g, b]) {
  const [rs, gs, bs] = [r, g, b].map((c) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  });
  return 0.2126 * rs + 0.7152 * gs + 0.0722 * bs;
}

function contrastRatio(hexA, hexB) {
  const l1 = relativeLuminance(hexToRgb(hexA));
  const l2 = relativeLuminance(hexToRgb(hexB));
  const lighter = Math.max(l1, l2);
  const darker = Math.min(l1, l2);
  return (lighter + 0.05) / (darker + 0.05);
}

const textRoles = ["text", "mutedText", "accent", "info", "attention", "success", "danger"];
const uiRoles = ["border", "accent", "info", "attention", "success", "danger"];
const backgrounds = ["background", "surface", "surfaceElevated"];

let failed = 0;
const lines = [
  "# Beehaven Style Guide & WCAG AA Contrast Report",
  "",
  "Generated automatically by `node scripts/check-contrast.mjs` from `design/tokens.json`.",
  "",
  "## Contrast Thresholds (WCAG 2.1 AA)",
  "- **Normal text & status labels:** >= `4.5:1` against `background`, `surface`, and `surfaceElevated`",
  "- **Borders, icons, and focus rings:** >= `3.0:1` against `background`, `surface`, and `surfaceElevated`",
  "",
];

for (const [themeName, palette] of Object.entries(tokens.themes)) {
  lines.push(`## Theme: \`${themeName}\``);
  lines.push("");
  lines.push("| Foreground Role | Hex | Background Role | Hex | Ratio | Target | Pass? |");
  lines.push("| --- | --- | --- | --- | --- | --- | --- |");

  for (const bgRole of backgrounds) {
    for (const fgRole of textRoles) {
      const ratio = contrastRatio(palette[fgRole], palette[bgRole]);
      const ok = ratio >= 4.5;
      if (!ok) failed++;
      lines.push(
        `| \`${fgRole}\` (text) | \`${palette[fgRole]}\` | \`${bgRole}\` | \`${palette[bgRole]}\` | **${ratio.toFixed(2)}:1** | >= 4.5:1 | ${ok ? "PASS ✓" : "FAIL ✗"} |`
      );
    }
    for (const uiRole of ["border"]) {
      const ratio = contrastRatio(palette[uiRole], palette[bgRole]);
      const ok = ratio >= 3.0;
      if (!ok) failed++;
      lines.push(
        `| \`${uiRole}\` (UI/focus) | \`${palette[uiRole]}\` | \`${bgRole}\` | \`${palette[bgRole]}\` | **${ratio.toFixed(2)}:1** | >= 3.0:1 | ${ok ? "PASS ✓" : "FAIL ✗"} |`
      );
    }
  }
  lines.push("");
}

const outPath = path.resolve(__dirname, "../docs/STYLE-GUIDE.md");
fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, lines.join("\n"), "utf8");

if (failed > 0) {
  console.error(`Contrast check FAILED with ${failed} violations.`);
  process.exit(1);
} else {
  console.log(`All WCAG AA contrast checks PASSED across dark and light themes. Wrote ${outPath}`);
}
