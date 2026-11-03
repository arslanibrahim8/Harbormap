// The Code tab: one file, read-only, with line numbers, colours and a marked range.

import { ApiError, file as fetchFile } from "./api.ts";
import { byId, clear, h } from "./dom.ts";
import { plural } from "./format.ts";
import { initialState, toHtml, tokenize } from "./highlight.ts";
import { getState, setState } from "./state.ts";
import type { SourceFile } from "./types.ts";

/** The source as one HTML string: a row per line with its number, the code coloured
 * by `highlight.ts` (which carries open comments and strings from line to line), and
 * the rows `from` to `to` marked. `from` of 0 marks nothing. Every piece of text is
 * escaped by `toHtml`. */
export function codeHtml(text: string, lang: string | null, from: number, to: number): string {
  const lines = text.split("\n");
  if (lines[lines.length - 1] === "") lines.pop();
  const last = to >= from ? to : from;
  let lex = initialState();
  let out = "";
  for (let i = 0; i < lines.length; i += 1) {
    const n = i + 1;
    const line = (lines[i] as string).replace(/\r$/, "");
    const t = tokenize(lang, line, lex);
    lex = t.state;
    const hit = from > 0 && n >= from && n <= last;
    out += `<div class="${hit ? "cl hit" : "cl"}" data-l="${n}"><span class="ln">${n}</span><span class="lt">${toHtml(t.tokens)}</span></div>`;
  }
  return out;
}

/** The file whose text is on screen (or was last fetched). */
let loaded: SourceFile | null = null;
let request = 0;

function pane(): HTMLElement {
  return byId("pane-code");
}

function empty(text: string, problem = false): void {
  const el = pane();
  clear(el);
  el.append(h("p", { class: problem ? "empty problem" : "empty" }, problem ? `⚠ ${text}` : text));
}

function paint(): void {
  const el = pane();
  const code = getState().code;
  if (code === null || loaded === null || loaded.path !== code.path) {
    empty("Pick a file on the map or search with /");
    return;
  }
  const head = h(
    "div",
    { class: "code-head" },
    h("span", { class: "code-path" }, loaded.path),
    h("span", { class: "chip" }, loaded.lang ?? "plain text"),
    h("span", { class: "chip" }, plural(loaded.lines, "line", "lines")),
  );
  if (code.from > 0) {
    const to = code.to >= code.from ? code.to : code.from;
    head.append(h("span", { class: "chip" }, to === code.from ? `line ${code.from}` : `lines ${code.from}–${to}`));
  }
  const body = h("div", { class: "code-body" });
  body.innerHTML = codeHtml(loaded.text, loaded.lang, code.from, code.to);
  clear(el);
  el.append(head);
  if (loaded.truncated) {
    el.append(
      h("p", { class: "code-note" }, "This file is long, so only the first part is shown."),
    );
  }
  el.append(body);
  body.querySelector(".cl.hit")?.scrollIntoView({ block: "center" });
}

/** Fetch `path` and draw it; the newest request wins. */
async function load(path: string): Promise<void> {
  const mine = ++request;
  try {
    const f = await fetchFile(path);
    if (mine !== request) return;
    loaded = f;
    paint();
  } catch (e) {
    if (mine !== request) return;
    loaded = null;
    if (e instanceof ApiError && (e.status === 404 || e.status === 403)) {
      empty("This file is gone from the project.", true);
    } else {
      empty("Could not load this file.", true);
    }
  }
}

/** Open `path` in the Code tab, marking lines `from` to `to` (1-based) and scrolling
 * them into view. The text is fetched once per file; `reloadCode` fetches it again. */
export async function showCode(path: string, from?: number, to?: number): Promise<void> {
  const start = from ?? 0;
  setState({ tab: "code", code: { path, from: start, to: to ?? start } });
  if (loaded !== null && loaded.path === path) {
    paint();
    return;
  }
  empty("Loading…");
  await load(path);
}

/** Fetch the file in the Code tab again, after it changed on disk. */
export async function reloadCode(): Promise<void> {
  const code = getState().code;
  if (code !== null) await load(code.path);
}

/** Draw the empty state; the tab itself is switched by tabs.ts. */
export function initCode(): void {
  paint();
}
