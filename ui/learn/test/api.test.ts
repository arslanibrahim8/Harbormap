import { test } from "node:test";
import assert from "node:assert/strict";
import { ApiError, getJSON, onUnauthorized, subscribe } from "../src/api.ts";

const realFetch = globalThis.fetch;
const realES = (globalThis as { EventSource?: unknown }).EventSource;

function answer(status: number, body: unknown = {}): void {
  globalThis.fetch = (() =>
    Promise.resolve(new Response(JSON.stringify(body), { status }))) as typeof fetch;
}

test("a 401 tells the unauthorized listeners and still rejects", async () => {
  let told = 0;
  onUnauthorized(() => (told += 1));
  answer(401);
  await assert.rejects(getJSON("/api/overview"), (e: unknown) => e instanceof ApiError && e.status === 401);
  assert.equal(told, 1);
  answer(404);
  await assert.rejects(getJSON("/api/group/x"));
  assert.equal(told, 1, "only a 401 means the session ended");
  answer(200, { v: 1 });
  assert.deepEqual(await getJSON("/api/tech"), { v: 1 });
  globalThis.fetch = realFetch;
});

/** A stand-in EventSource the test drives by hand. */
class FakeES {
  static last: FakeES;
  readyState = 1;
  private handlers = new Map<string, ((e: unknown) => void)[]>();
  readonly url: string;
  constructor(url: string) {
    this.url = url;
    FakeES.last = this;
  }
  addEventListener(name: string, fn: (e: unknown) => void): void {
    this.handlers.set(name, [...(this.handlers.get(name) ?? []), fn]);
  }
  emit(name: string, e: unknown = {}): void {
    for (const fn of this.handlers.get(name) ?? []) fn(e);
  }
  close(): void {
    this.readyState = 2;
  }
}

test("a malformed event is ignored and the next good one still arrives", () => {
  (globalThis as { EventSource?: unknown }).EventSource = FakeES;
  const seen: string[] = [];
  subscribe(
    (seq) => seen.push(`hello ${seq}`),
    (d) => seen.push(`facts ${d.seq}`),
    () => {},
  );
  const es = FakeES.last;
  es.emit("hello", { data: "{not json" });
  es.emit("facts", { data: "" });
  es.emit("hello", { data: '{"seq":3}' });
  es.emit("facts", { data: '{"seq":4}' });
  assert.deepEqual(seen, ["hello 3", "facts 4"]);
  (globalThis as { EventSource?: unknown }).EventSource = realES;
});

test("a stream the browser gave up on probes the server, so a 401 reaches the listeners", async () => {
  (globalThis as { EventSource?: unknown }).EventSource = FakeES;
  let told = 0;
  onUnauthorized(() => (told += 1));
  answer(401);
  const states: boolean[] = [];
  subscribe(() => {}, () => {}, (live) => states.push(live));
  const es = FakeES.last;
  es.readyState = 2; // CLOSED: the browser does not retry after a 401
  es.emit("error");
  await new Promise((r) => setTimeout(r, 0));
  assert.deepEqual(states, [false]);
  assert.ok(told >= 1);
  globalThis.fetch = realFetch;
  (globalThis as { EventSource?: unknown }).EventSource = realES;
});
