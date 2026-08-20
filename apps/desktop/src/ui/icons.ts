// The icon set.
//
// Every glyph is built with createElementNS from a table of path data, not
// assembled as an HTML string. The paths here are ours and could not carry
// anything hostile, but the rule this codebase holds -- nothing sets
// innerHTML, anywhere -- is only worth having if it has no exceptions to
// remember. A single helper that always builds nodes is also what makes an
// icon safe to hand a caller that got its name from a plugin.
//
// One geometry for all of them: a 24x24 box, stroked rather than filled,
// with the weight set in CSS so a glyph stays a hairline at any zoom.

const SVG_NS = "http://www.w3.org/2000/svg";

/** One primitive inside an icon. */
type Shape = [tag: string, attrs: Record<string, string | number>];

const p = (d: string): Shape => ["path", { d }];
const circle = (cx: number, cy: number, r: number): Shape => ["circle", { cx, cy, r }];
const rect = (x: number, y: number, width: number, height: number, rx = 2): Shape => [
  "rect",
  { x, y, width, height, rx },
];
// A handful of emblems are too small to survive being outlined -- a play
// triangle at 16px closes up into a blob. Those are filled.
const solid = (d: string): Shape => ["path", { d, fill: "currentColor", stroke: "none" }];

const ICONS: Record<string, Shape[]> = {
  // The brand mark.
  //
  // The same path data tools/brand.py rasterizes onto the application icon,
  // at the same 2.0 weight, so the mark on the library and the mark in the
  // dock are one drawing rather than two that resemble each other.
  // tools/check_brand.py fails the build if they drift.
  //
  // Its siblings live in lockbox (brandLockbox) and hearth (brandHearth) and
  // are built the same way: a container holding one emblem. A box with a
  // keyhole there; an arch with a fire there; a CRT with a play triangle
  // here.
  brandDen: [
    ["path", {
      d: "M 5 7 H 19 A 2 2 0 0 1 21 9 V 16 "
        + "A 2 2 0 0 1 19 18 H 5 A 2 2 0 0 1 3 16 "
        + "V 9 A 2 2 0 0 1 5 7 Z",
      "stroke-width": 2,
    }],
    ["path", { d: "M 9.5 7 V 4", "stroke-width": 2 }],
    ["path", { d: "M 14.5 7 V 4", "stroke-width": 2 }],
    solid("M 10.4 10.4 L 14.6 12.5 L 10.4 14.6 Z"),
  ],

  // chrome
  menu: [p("M4 6h16"), p("M4 12h16"), p("M4 18h16")],
  close: [p("M18 6 6 18"), p("M6 6l12 12")],
  plus: [p("M12 5v14"), p("M5 12h14")],
  check: [p("M20 6 9 17l-5-5")],
  search: [circle(11, 11, 8), p("m21 21-4.3-4.3")],
  chevronDown: [p("m6 9 6 6 6-6")],
  chevronRight: [p("m9 18 6-6-6-6")],
  back: [p("M19 12H5"), p("m12 19-7-7 7-7")],

  // den-specific
  play: [solid("M8 5.5v13l11-6.5z")],
  folder: [
    p("M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"),
  ],
  gamepad: [
    p("M6 12h4"),
    p("M8 10v4"),
    circle(15.5, 11, 0.5),
    circle(17.5, 13, 0.5),
    p("M17.32 5H6.68a4 4 0 0 0-3.98 3.6A14.5 14.5 0 0 0 2.5 11.5a6.5 6.5 0 0 0 6.5 6.5c1.3 0 2.6-.4 3.6-1.1a1 1 0 0 1 .9-.1l1.1.6a1 1 0 0 0 .8.1l1-.4a5.6 5.6 0 0 1 2.6-.6h.6a5.5 5.5 0 0 0 2.2-.5l.7-.4a.7.7 0 0 0 .4-.6v-1.2a2.4 2.4 0 0 0-1.7-2.3l-.8-.2a1.2 1.2 0 0 1-.9-1.2V9a4 4 0 0 0-4-4z"),
  ],
  tv: [
    rect(3, 5, 18, 13, 2),
    p("M8 21h8"),
    p("M12 18v3"),
  ],
  box: [
    p("M21 8 12 3 3 8v8l9 5 9-5z"),
    p("M3 8l9 5 9-5"),
    p("M12 13v8"),
  ],
  wrench: [
    p("M14.7 6.3a4 4 0 0 0-5.4 5.4L4 17v3h3l5.3-5.3a4 4 0 0 0 5.4-5.4l-2.6 2.6-2.4-.6-.6-2.4z"),
  ],
  alert: [
    p("M12 9v4"),
    p("M12 17h.01"),
    p("M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z"),
  ],
  save: [
    p("M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z"),
    p("M17 21v-8H7v8"),
    p("M7 3v5h8"),
  ],
  refresh: [
    p("M21 12a9 9 0 1 1-2.6-6.3"),
    p("M21 3v6h-6"),
  ],
  eject: [
    p("M5 15h14"),
    p("M12 4l-7 8h14z"),
    p("M6 19h12"),
  ],
  hardDrive: [
    p("M22 12H2"),
    p("M5.5 17h.01"),
    p("M8.5 17h.01"),
    p("M5.5 5H9l2 3h8a2 2 0 0 1 2 2v5H3V7a2 2 0 0 1 2.5-2z"),
  ],
};

/** Build an SVG node for a named icon, or null if the name is unknown. */
export function icon(name: string): SVGSVGElement | null {
  const shapes = ICONS[name];
  if (!shapes) return null;
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("class", "icon");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("focusable", "false");
  for (const [tag, attrs] of shapes) {
    const el = document.createElementNS(SVG_NS, tag);
    for (const [key, value] of Object.entries(attrs)) {
      el.setAttribute(key, String(value));
    }
    svg.appendChild(el);
  }
  return svg;
}
