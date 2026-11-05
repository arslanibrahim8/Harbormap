import { test } from "node:test";
import assert from "node:assert/strict";
import { initialState, tokenize, toHtml } from "../src/highlight.ts";

const kinds = (r: { tokens: { kind: string }[] }) => r.tokens.map((t) => t.kind);

test("a string holding // is not a comment, a later // is", () => {
  const r = tokenize("typescript", 'const x = "a // b"; // c', initialState());
  const meaningful = r.tokens.filter((t) => t.kind !== "punct" && t.text.trim() !== "");
  assert.deepEqual(
    meaningful.map((t) => [t.kind, t.text]),
    [
      ["kw", "const"],
      ["plain", "x"],
      ["str", '"a // b"'],
      ["com", "// c"],
    ],
  );
  // The tokens put back together are the original line.
  assert.equal(r.tokens.map((t) => t.text).join(""), 'const x = "a // b"; // c');
});

test("a block comment opened on one line stays a comment on the next", () => {
  const a = tokenize("typescript", "let a = 1; /* start", initialState());
  assert.equal(a.tokens.at(-1)?.kind, "com");
  const b = tokenize("typescript", "still inside */ let b", a.state);
  assert.equal(b.tokens[0]?.kind, "com");
  assert.equal(b.tokens[0]?.text, "still inside */");
  assert.ok(b.tokens.some((t) => t.kind === "kw" && t.text === "let"));
  const c = tokenize("typescript", "let c", b.state);
  assert.equal(c.tokens[0]?.kind, "kw");
});

test("a block comment with no end covers the whole next line", () => {
  const a = tokenize("rust", "/* open", initialState());
  const b = tokenize("rust", "fn not_code() {}", a.state);
  assert.deepEqual(kinds(b), ["com"]);
});

test("a template string spans lines", () => {
  const a = tokenize("typescript", "const s = `one ${", initialState());
  assert.equal(a.tokens.at(-1)?.kind, "str");
  const b = tokenize("typescript", "two // not a comment", a.state);
  assert.deepEqual(kinds(b), ["str"]);
  const c = tokenize("typescript", "end` + 1", b.state);
  assert.equal(c.tokens[0]?.kind, "str");
  assert.equal(c.tokens[0]?.text, "end`");
  assert.ok(c.tokens.some((t) => t.kind === "num" && t.text === "1"));
});

test("toHtml escapes markup characters", () => {
  const html = toHtml([{ kind: "plain", text: `<script>"a" & 'b'</script>` }]);
  assert.ok(!html.includes("<script>"));
  assert.ok(html.includes("&lt;script&gt;"));
  assert.ok(html.includes("&amp;"));
  assert.ok(html.includes("&quot;a&quot;"));
  assert.ok(html.includes("&#39;b&#39;"));
});

test("toHtml wraps non-plain tokens in a class span", () => {
  const html = toHtml([
    { kind: "kw", text: "const" },
    { kind: "plain", text: " x" },
  ]);
  assert.equal(html, '<span class="tk-kw">const</span> x');
});

test("an unknown language is one plain token", () => {
  const r = tokenize("brainfuck", "+++[->+<]", initialState());
  assert.deepEqual(r.tokens, [{ kind: "plain", text: "+++[->+<]" }]);
  const n = tokenize(null, "anything // here", initialState());
  assert.deepEqual(n.tokens, [{ kind: "plain", text: "anything // here" }]);
});

test("python def is a keyword and # starts a comment", () => {
  const r = tokenize("python", "def total(items):  # sum", initialState());
  assert.deepEqual(r.tokens[0], { kind: "kw", text: "def" });
  const fn = r.tokens.find((t) => t.text === "total");
  assert.equal(fn?.kind, "fn");
  assert.equal(r.tokens.at(-1)?.kind, "com");
});

test("capitalised names are types, numbers are numbers", () => {
  const r = tokenize("go", "var n Order = 42", initialState());
  assert.ok(r.tokens.some((t) => t.kind === "type" && t.text === "Order"));
  assert.ok(r.tokens.some((t) => t.kind === "num" && t.text === "42"));
  assert.ok(r.tokens.some((t) => t.kind === "kw" && t.text === "var"));
});

test("an empty line gives no tokens and keeps the state", () => {
  const a = tokenize("typescript", "/* x", initialState());
  const b = tokenize("typescript", "", a.state);
  assert.deepEqual(b.tokens, []);
  assert.deepEqual(b.state, a.state);
});

test("a very long line is left plain", () => {
  const line = `const a = "${"x".repeat(5000)}";`;
  const r = tokenize("typescript", line, initialState());
  assert.deepEqual(r.tokens, [{ kind: "plain", text: line }]);
});
