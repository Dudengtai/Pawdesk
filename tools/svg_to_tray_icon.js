// Legacy line-art tray mark. The cow-cat avatar is now packed by:
//   python tools/pack_tray_icon.py
//
// This script is kept so the old SVG can still be rasterized for comparison;
// it writes assets/tray/_gen/icon_from_svg.png and will not overwrite icon.png.

const fs = require("fs");
const path = require("path");
const { Resvg } = require("@resvg/resvg-js");

const root = path.resolve(__dirname, "..");
const outDir = path.join(root, "assets", "tray");
const src = path.join(outDir, "cat.svg");
const outPng = path.join(outDir, "_gen", "icon_from_svg.png");

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

fs.mkdirSync(path.dirname(outPng), { recursive: true });
fs.writeFileSync(outPng, png);
console.log(`wrote ${outPng} (${png.length} bytes, ${SIZE}px)`);
