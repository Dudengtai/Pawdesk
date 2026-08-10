// Rasterize assets/tray/cat.svg → assets/tray/icon.png
// Usage:
//   npm.cmd install --no-save @resvg/resvg-js
//   node tools/svg_to_tray_icon.js
//
// Source of truth: assets/tray/cat.svg

const fs = require("fs");
const path = require("path");
const { Resvg } = require("@resvg/resvg-js");

const root = path.resolve(__dirname, "..");
const outDir = path.join(root, "assets", "tray");
const src = path.join(outDir, "cat.svg");
const outPng = path.join(outDir, "icon.png");

if (!fs.existsSync(src)) {
  console.error(`missing source SVG: ${src}`);
  process.exit(1);
}

const SIZE = 64;
// Soft plate so the line-art cat stays readable on light & dark taskbars.
const plate = `
  <circle cx="12" cy="12" r="11.2" fill="#FFF8F5" stroke="#1C191733" stroke-width="0.6"/>
`;

let svg = fs.readFileSync(src, "utf8");
// Force an explicit stroke color and slightly thicker lines at small sizes.
svg = svg
  .replace(/currentColor/g, "#1C1917")
  .replace(/stroke-width="1\.5"/g, 'stroke-width="1.7"')
  .replace(/<svg([^>]*)>/, `<svg$1>${plate}`);

const resvg = new Resvg(svg, {
  fitTo: { mode: "width", value: SIZE },
  background: "rgba(0,0,0,0)",
});
const png = resvg.render().asPng();

fs.mkdirSync(outDir, { recursive: true });
fs.writeFileSync(outPng, png);
console.log(`wrote ${outPng} (${png.length} bytes, ${SIZE}px)`);
