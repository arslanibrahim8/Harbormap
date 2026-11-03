// Thin client for the Harbormap Learn server: same-origin JSON over fetch, and the
// live event stream over EventSource. The token travels in a cookie the server sets
// on the first page load, so no call here handles it.

import type {
  CardPayload,
  FactsDelta,
  GroupView,
  Journeys,
  Overview,
  Search,
  SourceFile,
  Tech,
} from "./types.ts";

/** A non-2xx answer from the server. */
export class ApiError extends Error {
  readonly status: number;

  constructor(status: number, path: string) {
    super(`${path} answered ${status}`);
    this.name = "ApiError";
    this.status = status;
  }
}

const unauthorizedListeners: (() => void)[] = [];

/** Call `fn` whenever the server answers 401: the cookie is gone or belongs to
 * another run, so nothing on this page will load again until the user opens the
 * link that `harbormap learn` printed. */
export function onUnauthorized(fn: () => void): void {
  unauthorizedListeners.push(fn);
}

/** GET `path` on the page's own origin and parse the JSON body. */
export async function getJSON<T>(path: string): Promise<T> {
  const res = await fetch(path, { credentials: "same-origin" });
  if (res.status === 401) for (const fn of unauthorizedListeners) fn();
  if (!res.ok) throw new ApiError(res.status, path);
  return (await res.json()) as T;
}

/** `JSON.parse` of an event's data, or `null` when it is not JSON, so one bad event
 * cannot break the handling of the ones after it. */
function parseEvent<T>(e: Event): T | null {
  try {
    return JSON.parse((e as MessageEvent<string>).data) as T;
  } catch {
    return null;
  }
}

export function overview(): Promise<Overview> {
  return getJSON<Overview>("/api/overview");
}

export function group(id: string): Promise<GroupView> {
  return getJSON<GroupView>(`/api/group/${encodeURIComponent(id)}`);
}

/** The card of a file, or of one symbol in it; `view` is `overview` or a group id,
 * and asks for the links of the file in that view. */
export function card(path: string, symbol?: string, view?: string): Promise<CardPayload> {
  const q = new URLSearchParams({ path });
  if (symbol !== undefined) q.set("symbol", symbol);
  if (view !== undefined) q.set("view", view);
  return getJSON<CardPayload>(`/api/card?${q}`);
}

export function file(path: string): Promise<SourceFile> {
  return getJSON<SourceFile>(`/api/file?${new URLSearchParams({ path })}`);
}

export function search(q: string): Promise<Search> {
  return getJSON<Search>(`/api/search?${new URLSearchParams({ q })}`);
}

export function journeys(): Promise<Journeys> {
  return getJSON<Journeys>("/api/journeys");
}

export function tech(): Promise<Tech> {
  return getJSON<Tech>("/api/tech");
}

/** Follow the live stream. `onHello` gets the current seq on every (re)connect,
 * `onFacts` each delta, `onState` false while the connection is down and true when
 * it is up. EventSource reconnects by itself. Returns a function that closes it. */
export function subscribe(
  onHello: (seq: number) => void,
  onFacts: (d: FactsDelta) => void,
  onState: (live: boolean) => void,
): () => void {
  const es = new EventSource("/api/events");
  es.addEventListener("open", () => onState(true));
  es.addEventListener("error", () => {
    onState(false);
    // A browser retries a dropped stream but gives up (CLOSED) on a refusal such as
    // a 401, and an event stream does not say which status it saw. Ask with a plain
    // request, which does: a 401 reaches the `onUnauthorized` listeners.
    if (es.readyState === 2) getJSON("/api/journeys").catch(() => {});
  });
  es.addEventListener("hello", (e) => {
    const data = parseEvent<{ seq: number }>(e);
    if (data !== null) onHello(data.seq);
  });
  es.addEventListener("facts", (e) => {
    const data = parseEvent<FactsDelta>(e);
    if (data !== null) onFacts(data);
  });
  return () => es.close();
}
