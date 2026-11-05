// SVG element helpers for the Map view. Like `h` in dom.ts: string children become
// text nodes and attributes go through `setAttribute`, so a file name or a path can
// never turn into markup. Nothing here runs until a function is called.

const NS = "http://www.w3.org/2000/svg";

/** Build an SVG element. Numbers are accepted as attribute values. */
export function s(
  tag: string,
  attrs: Record<string, string | number> = {},
  ...children: (Node | string)[]
): SVGElement {
  const el = document.createElementNS(NS, tag);
  for (const [name, value] of Object.entries(attrs)) el.setAttribute(name, String(value));
  el.append(...children);
  return el;
}

/** A `<text>` at (`x`, `y`); `text` is shown literally. */
export function text(x: number, y: number, cls: string, content: string): SVGElement {
  return s("text", { x, y, class: cls }, content);
}

/** A `<title>` child: the browser's tooltip for its parent. */
export function title(content: string): SVGElement {
  return s("title", {}, content);
}

/** The arrowhead markers; one per colour, because `context-stroke` is not yet
 * everywhere. They live in the SVG's `<defs>`. */
export function defs(): SVGElement {
  const head = (id: string, cls: string): SVGElement =>
    s(
      "marker",
      {
        id,
        viewBox: "0 0 10 10",
        refX: 9,
        refY: 5,
        markerWidth: 9,
        markerHeight: 9,
        markerUnits: "userSpaceOnUse",
        orient: "auto",
      },
      s("path", { d: "M0 1L10 5L0 9z", class: cls }),
    );
  return s("defs", {}, head("head", "head"), head("head-sel", "head sel"));
}

/** A `viewBox` attribute value. */
export function viewBoxOf(v: { x: number; y: number; w: number; h: number }): string {
  return `${v.x} ${v.y} ${v.w} ${v.h}`;
}
