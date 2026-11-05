// A small line-by-line syntax highlighter for the source view. It is a lexer, not a
// parser: it knows keywords, strings, comments and numbers of a few languages and
// carries one thing across lines (an open block comment or multi-line string).
// Any other language comes back as plain text. No DOM, so `node --test` runs it.

export type TokenKind = "kw" | "str" | "com" | "num" | "fn" | "type" | "punct" | "plain";

export interface Token {
  kind: TokenKind;
  text: string;
}

/** What is open at the end of a line: `null` for ordinary code, otherwise a block
 * comment or a multi-line string, the text that closes it, and whether a backslash
 * escapes inside it (not in comments or Go raw strings). */
export interface LexState {
  open: { kind: "com" | "str"; close: string; esc: boolean } | null;
}

/** The state at the start of a file. */
export function initialState(): LexState {
  return { open: null };
}

interface Language {
  keywords: Set<string>;
  /** Text that starts a comment to the end of the line. */
  line: string;
  /** Block comments `/* ... *\/`. */
  block: boolean;
  /** Triple-quoted strings (`"""` and, for Python, `'''`) that may span lines. */
  triple: string[];
  /** Backtick strings that may span lines (template strings, Go raw strings). */
  backtick: boolean;
  /** Rust writes lifetimes with a lone `'`. */
  lifetimes: boolean;
}

const words = (s: string): Set<string> => new Set(s.split(/\s+/).filter(Boolean));

const JS_KEYWORDS = words(`
  as async await break case catch class const continue debugger default delete do else
  enum export extends false finally for from function get if implements import in
  instanceof interface let new null of private protected public readonly return set
  static super switch this throw true try type typeof undefined var void while with yield
  abstract declare namespace satisfies keyof infer is override`);

const JS: Language = {
  keywords: JS_KEYWORDS,
  line: "//",
  block: true,
  triple: [],
  backtick: true,
  lifetimes: false,
};

const LANGUAGES: Record<string, Language> = {
  typescript: JS,
  tsx: JS,
  javascript: JS,
  python: {
    keywords: words(`
      False None True and as assert async await break class continue def del elif else
      except finally for from global if import in is lambda nonlocal not or pass raise
      return try while with yield match case self`),
    line: "#",
    block: false,
    triple: ['"""', "'''"],
    backtick: false,
    lifetimes: false,
  },
  go: {
    keywords: words(`
      break case chan const continue default defer else fallthrough for func go goto if
      import interface map package range return select struct switch type var true
      false nil iota`),
    line: "//",
    block: true,
    triple: [],
    backtick: true,
    lifetimes: false,
  },
  rust: {
    keywords: words(`
      as async await break const continue crate dyn else enum extern false fn for if impl
      in let loop match mod move mut pub ref return self Self static struct super trait
      true type unsafe use where while`),
    line: "//",
    block: true,
    triple: [],
    backtick: false,
    lifetimes: true,
  },
  java: {
    keywords: words(`
      abstract assert boolean break byte case catch char class const continue default do
      double else enum extends final finally float for goto if implements import
      instanceof int interface long native new null package private protected public
      return short static strictfp super switch synchronized this throw throws transient
      true false try var void volatile while record sealed permits yield`),
    line: "//",
    block: true,
    triple: ['"""'],
    backtick: false,
    lifetimes: false,
  },
  csharp: {
    keywords: words(`
      abstract as async await base bool break byte case catch char checked class const
      continue decimal default delegate do double else enum event explicit extern false
      finally fixed float for foreach goto if implicit in int interface internal is lock
      long namespace new null object operator out override params private protected
      public readonly record ref return sbyte sealed short sizeof stackalloc static
      string struct switch this throw true try typeof uint ulong unchecked unsafe
      ushort using var virtual void volatile while yield`),
    line: "//",
    block: true,
    triple: ['"""'],
    backtick: false,
    lifetimes: false,
  },
};

/** Longer lines are not highlighted: scanning them would stall the page. */
const MAX_LINE = 2000;

const NUMBER = /^(?:0[xXbBoO][0-9a-fA-F_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)/;
const IDENT = /^[A-Za-z_$][\w$]*/;
const SPACE = /^\s+/;
const RUST_CHAR = /^'(?:\\.[^']*|[^\\'])'/;

/** The index just after the end of the string that starts at `from` (after its
 * opening quote) and closes with `quote`, or the line length if it never closes. */
function endOfQuoted(line: string, from: number, quote: string): number {
  let i = from;
  while (i < line.length) {
    if (line[i] === "\\") i += 2;
    else if (line.startsWith(quote, i)) return i + quote.length;
    else i += 1;
  }
  return line.length;
}

/** Scan an open block comment or multi-line string from `from`: where it ends on
 * this line, and what is still open after it. */
function continueOpen(
  line: string,
  from: number,
  open: NonNullable<LexState["open"]>,
): { end: number; open: LexState["open"] } {
  let i = from;
  while (i < line.length) {
    if (open.esc && line[i] === "\\") i += 2;
    else if (line.startsWith(open.close, i)) return { end: i + open.close.length, open: null };
    else i += 1;
  }
  return { end: line.length, open };
}

/** Split one line into tokens. `state` is what the previous line left open; the
 * result carries what this line leaves open. Joining the token texts gives `line`. */
export function tokenize(
  lang: string | null,
  line: string,
  state: LexState,
): { tokens: Token[]; state: LexState } {
  const spec = lang === null ? undefined : LANGUAGES[lang];
  // Unknown language, or a minified line nobody reads token by token: plain text.
  if (spec === undefined || (state.open === null && line.length > MAX_LINE)) {
    return { tokens: line === "" ? [] : [{ kind: "plain", text: line }], state };
  }

  const tokens: Token[] = [];
  const push = (kind: TokenKind, text: string) => {
    if (text !== "") tokens.push({ kind, text });
  };
  let pos = 0;
  let open = state.open;

  if (open !== null) {
    const r = continueOpen(line, 0, open);
    push(open.kind, line.slice(0, r.end));
    pos = r.end;
    open = r.open;
  }

  while (pos < line.length) {
    const rest = line.slice(pos);
    const c = rest[0] as string;

    let m = SPACE.exec(rest);
    if (m !== null) {
      push("plain", m[0]);
      pos += m[0].length;
      continue;
    }

    if (rest.startsWith(spec.line)) {
      push("com", rest);
      break;
    }

    if (spec.block && rest.startsWith("/*")) {
      const r = continueOpen(line, pos + 2, { kind: "com", close: "*/", esc: false });
      push("com", line.slice(pos, r.end));
      pos = r.end;
      open = r.open;
      continue;
    }

    const triple = spec.triple.find((q) => rest.startsWith(q));
    if (triple !== undefined) {
      const r = continueOpen(line, pos + 3, { kind: "str", close: triple, esc: true });
      push("str", line.slice(pos, r.end));
      pos = r.end;
      open = r.open;
      continue;
    }

    if (c === "`" && spec.backtick) {
      const r = continueOpen(line, pos + 1, { kind: "str", close: "`", esc: lang !== "go" });
      push("str", line.slice(pos, r.end));
      pos = r.end;
      open = r.open;
      continue;
    }

    if (c === '"' || (c === "'" && !spec.lifetimes)) {
      const end = endOfQuoted(line, pos + 1, c);
      push("str", line.slice(pos, end));
      pos = end;
      continue;
    }

    if (c === "'") {
      // Rust: a char literal is a string, a lifetime (`'a`) is only a name.
      m = RUST_CHAR.exec(rest);
      if (m !== null) {
        push("str", m[0]);
        pos += m[0].length;
      } else {
        push("punct", c);
        pos += 1;
      }
      continue;
    }

    m = NUMBER.exec(rest);
    if (m !== null) {
      push("num", m[0]);
      pos += m[0].length;
      continue;
    }

    m = IDENT.exec(rest);
    if (m !== null) {
      const word = m[0];
      const after = rest.slice(word.length);
      let kind: TokenKind = "plain";
      if (spec.keywords.has(word)) kind = "kw";
      else if (/^\s*\(/.test(after)) kind = "fn";
      else if (/^[A-Z]/.test(word)) kind = "type";
      push(kind, word);
      pos += word.length;
      continue;
    }

    push("punct", c);
    pos += 1;
  }

  return { tokens, state: { open } };
}

const ESCAPES: Record<string, string> = {
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  '"': "&quot;",
  "'": "&#39;",
};

/** The tokens as safe HTML: text escaped, every kind but plain in a `tk-<kind>` span. */
export function toHtml(tokens: Token[]): string {
  let out = "";
  for (const t of tokens) {
    const text = t.text.replace(/[&<>"']/g, (ch) => ESCAPES[ch] as string);
    out += t.kind === "plain" ? text : `<span class="tk-${t.kind}">${text}</span>`;
  }
  return out;
}
