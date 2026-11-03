// The drawer on the right: the card of a file or of one function. The row builders
// (`fileCardRows`, `symbolCardRows`) are pure and hold every rule about what the
// card shows at each level; the render functions only turn rows into elements.

import { ApiError, card as fetchCard } from "./api.ts";
import { showCode } from "./code.ts";
import { byId, clear, h, isTyping, notify } from "./dom.ts";
import { calledFrom, fileName, usedFrom } from "./format.ts";
import type { Level } from "./format.ts";
import { currentView, getState, onChange, setState } from "./state.ts";
import type { CardPayload, FileCard, SymbolCard } from "./types.ts";

/** Beginner lists this many importers and says how many more there are. */
const BEGINNER_USED_BY = 5;

/** A line that names a file; `path` is set when the file can be opened. */
export interface LinkRow {
  text: string;
  path: string | null;
}

export interface FileCardRows {
  title: string;
  path: string;
  /** Until the prose version arrives the card says so in one fixed line. */
  lead: string;
  usedBy: { summary: string; rows: LinkRow[]; more: string | null };
  jobs: { name: string; kind: string; calls: string; exported: boolean; symbol: string }[];
  uses: LinkRow[];
  details: { label: string; value: string }[];
  symbolLines: { name: string; lines: string }[];
}

export interface SymbolCardRows {
  title: string;
  kind: string;
  /** Builder only, and only when the file has one. */
  signature: string | null;
  callersSummary: string;
  callers: string[];
  callees: string[];
  unresolved: string[];
  details: { label: string; value: string }[];
  path: string;
  from: number;
  to: number;
}

/** `12–30`, or `12` for a one-line symbol. */
export function lineRange(l0: number, l1: number): string {
  return l0 === l1 ? `${l0}` : `${l0}–${l1}`;
}

/** What the card of a file shows. Beginner shortens the list of importers; the job
 * names are never cut, at any level. */
export function fileCardRows(card: FileCard, level: Level): FileCardRows {
  const limit = level === "beginner" ? BEGINNER_USED_BY : card.used_by.length;
  const shown = card.used_by.slice(0, limit);
  const rest = card.used_by.length - shown.length;
  return {
    title: fileName(card.path),
    path: card.path,
    lead: "Written descriptions arrive with the prose version.",
    usedBy: {
      summary: usedFrom(level, card.used_by.length),
      rows: shown.map((p) => ({ text: p, path: p })),
      more: rest > 0 ? `and ${rest} more` : null,
    },
    jobs: card.symbols.map((s) => ({
      name: s.name,
      kind: s.kind,
      calls: calledFrom(level, s.called_from),
      exported: s.exported,
      symbol: s.name,
    })),
    // A package is written `name (external)`: there is no file to open.
    uses: card.uses.map((u) => ({ text: u, path: u.endsWith(" (external)") ? null : u })),
    details: [
      { label: "Lines", value: String(card.lines) },
      { label: "Language", value: card.lang ?? "unknown" },
      { label: "Class", value: card.class },
      { label: "Parse status", value: card.parse_status },
      { label: "Unresolved references", value: String(card.unresolved) },
    ],
    symbolLines: card.symbols.map((s) => ({ name: s.name, lines: lineRange(s.l0, s.l1) })),
  };
}

/** What the card of one function shows. */
export function symbolCardRows(card: SymbolCard, level: Level): SymbolCardRows {
  return {
    title: card.name,
    kind: card.kind,
    signature: level === "builder" && card.sig !== "" ? card.sig : null,
    callersSummary: calledFrom(level, card.callers.length),
    callers: card.callers,
    callees: card.callees,
    unresolved: card.unresolved_calls,
    details: [
      { label: "Lines", value: lineRange(card.l0, card.l1) },
      { label: "File", value: card.path },
    ],
    path: card.path,
    from: card.l0,
    to: card.l1,
  };
}

// ---- Loading ---------------------------------------------------------------

/** What the drawer was asked to show; kept so a level change or a file change can
 * draw or fetch it again. `view` is `overview` or a group id. */
export interface Target {
  path: string;
  symbol: string | null;
  view: string;
}

/** Everything the card loader needs from outside, so it runs under `node --test`
 * with plain stand-ins. */
export interface CardDeps {
  fetchCard(t: Target): Promise<CardPayload>;
  /** The model version of the overview on screen. */
  seq(): number;
  /** A card the user asked for is on its way. */
  loading(): void;
  show(p: CardPayload): void;
  /** The server no longer knows the file (404). */
  gone(t: Target): void;
  /** The request failed for another reason (only for a card the user asked for). */
  failed(): void;
}

/** How many times a card is asked for when each answer is older than the overview. */
const MAX_ATTEMPTS = 3;

/** The rules for getting a card onto the screen: the newest request wins, an
 * answer older than the overview is asked for again (a refresh that simply
 * dropped it would leave a stale card, because nothing else refetches), and a 404
 * means the file is gone. */
export function createCardLoader(deps: CardDeps) {
  let request = 0;
  let target: Target | null = null;

  async function load(t: Target, quiet: boolean): Promise<void> {
    const mine = ++request;
    target = t;
    if (!quiet) deps.loading();
    try {
      let p = await deps.fetchCard(t);
      if (mine !== request) return;
      // Still older after the last attempt: take it, the server is the authority.
      for (let n = 1; n < MAX_ATTEMPTS && p.seq < deps.seq(); n += 1) {
        p = await deps.fetchCard(t);
        if (mine !== request) return;
      }
      deps.show(p);
    } catch (e) {
      if (mine !== request) return;
      if (e instanceof ApiError && e.status === 404) deps.gone(t);
      else if (!quiet) deps.failed();
    }
  }

  return {
    load,
    /** Fetch the current target again, without a loading state. */
    refresh: (): Promise<void> => (target === null ? Promise.resolve() : load(target, true)),
    /** Forget the target and ignore anything still on its way. */
    close(): void {
      request += 1;
      target = null;
    },
    target: (): Target | null => target,
  };
}

// ---- Drawing ---------------------------------------------------------------

let payload: CardPayload | null = null;
const cardListeners: (() => void)[] = [];

/** The card on screen, or null when the drawer is closed or still loading. The map
 * reads the links of a file card from it. */
export function currentCard(): CardPayload | null {
  return payload;
}

/** Call `fn` whenever the card on screen changes (shown, refreshed or closed). */
export function onCard(fn: () => void): void {
  cardListeners.push(fn);
}

function cardChanged(): void {
  for (const fn of cardListeners) fn();
}
/** Which card the drawer last drew, so a redraw of the same card can keep the
 * Technical details fold open and the scroll position. */
let renderedKey: string | null = null;

function drawer(): HTMLElement {
  return byId("drawer");
}

function section(title: string, ...body: (Node | string)[]): HTMLElement {
  return h("section", { class: "dsec" }, h("h3", {}, title), ...body);
}

/** A line that opens a file's card when it names a file. */
function linkRow(row: LinkRow): HTMLElement {
  if (row.path === null) return h("li", { class: "row" }, h("span", { class: "mono" }, row.text));
  const path = row.path;
  const b = h("button", { class: "linkbtn mono", type: "button" }, row.text);
  b.addEventListener("click", () => void openFile(path, currentView()));
  return h("li", { class: "row" }, b);
}

function list(rows: HTMLElement[], none: string): HTMLElement {
  return rows.length === 0
    ? h("p", { class: "none" }, none)
    : h("ul", { class: "rows" }, ...rows);
}

function plainList(items: string[], none: string): HTMLElement {
  return list(
    items.map((t) => h("li", { class: "row" }, h("span", { class: "mono" }, t))),
    none,
  );
}

function detailsFold(rows: { label: string; value: string }[], extra?: HTMLElement): HTMLElement {
  const facts = h(
    "div",
    { class: "facts" },
    ...rows.map((r) =>
      h("div", { class: "fact" }, h("span", {}, r.label), h("code", {}, r.value)),
    ),
  );
  return h("details", { class: "fold" }, h("summary", {}, "Technical details"), facts, ...(extra ? [extra] : []));
}

function openInCode(path: string, from?: number, to?: number): HTMLElement {
  const b = h("button", { class: "btn pri", type: "button" }, "Open in Code");
  b.addEventListener("click", () => void showCode(path, from, to));
  return h("div", { class: "actions" }, b);
}

function closeButton(): HTMLElement {
  const b = h("button", { class: "close", type: "button", "aria-label": "Close" }, "×");
  b.addEventListener("click", closeDrawer);
  return b;
}

function renderFile(card: FileCard, level: Level): HTMLElement[] {
  const r = fileCardRows(card, level);
  const jobs = r.jobs.map((j) => {
    const b = h("button", { class: "linkbtn mono", type: "button" }, j.name);
    b.addEventListener("click", () => void openSymbol(card.path, j.symbol));
    return h(
      "li",
      { class: "row job" },
      b,
      h("span", { class: "kind" }, j.kind),
      ...(level === "builder" && j.exported ? [h("span", { class: "kind" }, "exported")] : []),
      h("span", { class: "calls" }, j.calls),
    );
  });
  const usedBy = list(r.usedBy.rows.map(linkRow), "No other file uses it yet.");
  if (r.usedBy.more !== null) usedBy.append(h("p", { class: "more" }, r.usedBy.more));
  const symbolLines = h(
    "div",
    { class: "facts" },
    ...r.symbolLines.map((s) =>
      h("div", { class: "fact" }, h("span", { class: "mono" }, s.name), h("code", {}, s.lines)),
    ),
  );
  return [
    h("div", { class: "kicker" }, "File"),
    h("h2", {}, r.title),
    h("div", { class: "chip mono pathchip" }, r.path),
    h("p", { class: "lead" }, r.lead),
    section("Who uses it", h("p", { class: "count" }, r.usedBy.summary), usedBy),
    section("Jobs inside", list(jobs, "Harbormap found no jobs inside this file.")),
    section("Uses", list(r.uses.map(linkRow), "It uses no other file.")),
    detailsFold(r.details, r.symbolLines.length > 0 ? symbolLines : undefined),
    openInCode(card.path),
  ];
}

function renderSymbol(card: SymbolCard, level: Level): HTMLElement[] {
  const r = symbolCardRows(card, level);
  const out: HTMLElement[] = [
    h("div", { class: "kicker" }, r.kind),
    h("h2", { class: "mono" }, r.title),
  ];
  if (r.signature !== null) out.push(h("pre", { class: "sig mono" }, r.signature));
  out.push(
    section("Who calls it", h("p", { class: "count" }, r.callersSummary), plainList(r.callers, "Nothing calls it yet.")),
    section("What it calls", plainList(r.callees, "It calls nothing Harbormap can follow.")),
  );
  if (r.unresolved.length > 0) {
    out.push(section("Calls Harbormap cannot follow yet", plainList(r.unresolved, "")));
  }
  out.push(detailsFold(r.details), openInCode(r.path, r.from, r.to));
  return out;
}

/** Put `nodes` in the drawer under the close button and show the drawer. */
function show(...nodes: HTMLElement[]): void {
  const el = drawer();
  clear(el);
  el.append(closeButton(), ...nodes);
  el.hidden = false;
}

function cardKey(p: CardPayload): string {
  return p.kind === "file" ? `f:${p.card.path}` : `s:${p.card.path}:${p.card.name}`;
}

function render(): void {
  if (payload === null) return;
  const level = getState().level;
  const key = cardKey(payload);
  const el = drawer();
  // The same card drawn again (level switch, live refresh) must not close a fold
  // the user opened or jump back to the top.
  const keep = key === renderedKey && !el.hidden;
  const wasOpen = keep && el.querySelector<HTMLDetailsElement>("details.fold")?.open === true;
  const top = el.scrollTop;
  show(...(payload.kind === "file" ? renderFile(payload.card, level) : renderSymbol(payload.card, level)));
  if (keep) {
    if (wasOpen) el.querySelector<HTMLDetailsElement>("details.fold")?.setAttribute("open", "");
    el.scrollTop = top;
  }
  renderedKey = key;
}

/** What the notice says when a card's file or function is no longer indexed. */
export function goneNotice(t: Target): string {
  return t.symbol === null ? "This file is gone from the project." : "This function is gone from the project.";
}

const loader = createCardLoader({
  fetchCard: (t) => fetchCard(t.path, t.symbol ?? undefined, t.view),
  seq: () => getState().seq,
  loading: () => {
    renderedKey = null;
    show(h("p", { class: "none" }, "Loading…"));
  },
  show: (p) => {
    payload = p;
    render();
    cardChanged();
  },
  gone: (t) => {
    closeDrawer();
    notify(goneNotice(t));
  },
  failed: () => {
    payload = null;
    renderedKey = null;
    show(h("p", { class: "problem" }, "⚠ Could not load this card."));
    cardChanged();
  },
});

/** Show the card of a file; `view` is `overview` or the id of the expanded group
 * the links should be counted in. */
export async function openFile(path: string, view: string): Promise<void> {
  setState({ selected: { path, symbol: null } });
  await loader.load({ path, symbol: null, view }, false);
}

/** Show the card of one function (or method, or class) of a file. */
export async function openSymbol(path: string, symbol: string): Promise<void> {
  setState({ selected: { path, symbol } });
  await loader.load({ path, symbol, view: currentView() }, false);
}

/** Close the drawer and clear the selection. */
export function closeDrawer(): void {
  loader.close();
  payload = null;
  renderedKey = null;
  drawer().hidden = true;
  setState({ selected: null });
  cardChanged();
}

/** Fetch the open card again, after the facts changed. Does nothing when the
 * drawer is closed. */
export async function refreshDrawer(): Promise<void> {
  await loader.refresh();
}

/** Wire the drawer: redraw on a level change, close on Escape. */
export function initDrawer(): void {
  onChange((s, prev) => {
    if (s.level !== prev.level) render();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !isTyping(e.target) && loader.target() !== null) closeDrawer();
  });
}
