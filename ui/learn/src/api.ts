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

/** GET `path` on the page's own origin and parse the JSON body. */
export async function getJSON<T>(path: string): Promise<T> {
  const res = await fetch(path, { credentials: "same-origin" });
  if (!res.ok) throw new ApiError(res.status, path);
  return (await res.json()) as T;
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
  es.addEventListener("error", () => onState(false));
  es.addEventListener("hello", (e) => {
    const data = JSON.parse((e as MessageEvent<string>).data) as { seq: number };
    onHello(data.seq);
  });
  es.addEventListener("facts", (e) => {
    onFacts(JSON.parse((e as MessageEvent<string>).data) as FactsDelta);
  });
  return () => es.close();
}
