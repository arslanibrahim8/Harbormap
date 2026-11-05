import { test } from "node:test";
import assert from "node:assert/strict";
import { codeHtml } from "../src/code.ts";

test("every line gets a number, and the trailing newline does not add a line", () => {
  const html = codeHtml("const a = 1;\nconst b = 2;\n", "typescript", 0, 0);
  assert.equal(html.split('class="cl"').length - 1, 2);
  assert.ok(html.includes('data-l="1"'));
  assert.ok(html.includes('data-l="2"'));
  assert.ok(!html.includes('data-l="3"'));
});

test("the range from-to is marked and nothing else is", () => {
  const html = codeHtml("a\nb\nc\nd\n", null, 2, 3);
  assert.ok(html.includes('class="cl" data-l="1"'));
  assert.ok(html.includes('class="cl hit" data-l="2"'));
  assert.ok(html.includes('class="cl hit" data-l="3"'));
  assert.ok(html.includes('class="cl" data-l="4"'));
});

test("source text is escaped", () => {
  const html = codeHtml('<script>alert("x")</script>', null, 0, 0);
  assert.ok(!html.includes("<script>"));
  assert.ok(html.includes("&lt;script&gt;"));
});

test("a block comment that spans lines is coloured on every line", () => {
  const html = codeHtml("/* one\ntwo */ let x = 1;", "typescript", 0, 0);
  assert.ok(html.includes('<span class="tk-com">/* one</span>'));
  assert.ok(html.includes('<span class="tk-com">two */</span>'));
});

test("carriage returns from Windows files are dropped", () => {
  const html = codeHtml("a\r\nb\r\n", null, 0, 0);
  assert.ok(!html.includes("\r"));
});

test("an empty file has no lines", () => {
  assert.equal(codeHtml("", "typescript", 0, 0), "");
});
