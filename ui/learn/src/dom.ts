// The few DOM helpers every panel module shares. Nothing here touches `document`
// until a function is called, so modules that import it still load under Node.

/** Build an element. Children that are strings become text nodes, so data can never
 * turn into markup; attributes are set with `setAttribute` (use `""` for a boolean
 * one such as `hidden`). */
export function h(
  tag: string,
  attrs: Record<string, string> = {},
  ...children: (Node | string)[]
): HTMLElement {
  const el = document.createElement(tag);
  for (const [name, value] of Object.entries(attrs)) el.setAttribute(name, value);
  el.append(...children);
  return el;
}

/** The element with this id; the page skeleton in index.html always has it. */
export function byId(id: string): HTMLElement {
  const el = document.getElementById(id);
  if (el === null) throw new Error(`#${id} is missing from the page`);
  return el;
}

/** Remove everything inside `el`. */
export function clear(el: HTMLElement): void {
  el.replaceChildren();
}

/** True when a key press came from a place where the user is typing text. */
export function isTyping(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)
  );
}

let noticeTimer: number | undefined;

/** Show a one-line notice above the tabs for a few seconds. */
export function notify(text: string): void {
  const el = byId("notice");
  el.textContent = text;
  el.hidden = false;
  window.clearTimeout(noticeTimer);
  noticeTimer = window.setTimeout(() => {
    el.hidden = true;
  }, 6000);
}
