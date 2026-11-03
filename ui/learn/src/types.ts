// The API contract between the Harbormap Learn server and this panel. Every type
// mirrors a JSON payload the Rust server sends (Learn L5a, "API contract"); change
// them together. All responses carry a top-level `v: 1`.

/** The part a box plays in the app, in lane order (shared sits below the lanes). */
export type Role = "see" | "work" | "keep" | "outside" | "shared";

export type NodeKind = "folder" | "token" | "files" | "more" | "shared" | "file";

/** One box on the map. Overview boxes are in world coordinates; the children of a
 * group are relative to the group's top-left corner. */
export interface Box {
  id: string;
  name: string;
  kind: NodeKind;
  role: Role;
  role_word: string;
  files: number;
  loc: number;
  x: number;
  y: number;
  w: number;
  h: number;
  expandable: boolean;
  /** True when the box is among the ones Beginner shows. */
  beginner: boolean;
  /** File nodes only. */
  path?: string;
  generated?: boolean;
}

/** A link between two boxes, weighted by how many references run along it. */
export interface Arrow {
  from: string;
  to: string;
  count: number;
}

/** A neighbour outside an expanded group that the group links to. */
export interface Chip {
  id: string;
  name: string;
  role: Role;
  out: number;
  in: number;
}

export interface Repo {
  name: string;
  branch: string | null;
  /** The first 7 characters of the commit id. */
  commit: string | null;
}

/** GET /api/overview */
export interface Overview {
  v: 1;
  seq: number;
  repo: Repo;
  updated_ms: number;
  boxes: Box[];
  arrows: Arrow[];
  hidden_arrows: number;
  excluded: { class: string; files: number }[];
  files_total: number;
}

/** GET /api/group/{id}; 404 when the id is unknown. */
export interface GroupView {
  v: 1;
  seq: number;
  id: string;
  name: string;
  role: Role;
  role_word: string;
  w: number;
  h: number;
  breadcrumb: { id: string; name: string }[];
  children: Box[];
  arrows: Arrow[];
  hidden_arrows: number;
  elsewhere: Chip[];
  elsewhere_more: number;
}

/** One line of a file card (hm_index::query::SymbolRow). */
export interface SymbolRow {
  /** `Owner.name`, or `name` for a top-level symbol. */
  name: string;
  kind: string;
  l0: number;
  l1: number;
  exported: boolean;
  called_from: number;
}

/** hm_index::query::FileCard */
export interface FileCard {
  path: string;
  lang: string | null;
  class: string;
  parse_status: string;
  lines: number;
  symbols: SymbolRow[];
  used_by: string[];
  uses: string[];
  unresolved: number;
}

/** hm_index::query::SymbolCard */
export interface SymbolCard {
  name: string;
  kind: string;
  path: string;
  l0: number;
  l1: number;
  sig: string;
  callers: string[];
  callees: string[];
  unresolved_calls: string[];
}

/** GET /api/card?path=P[&view=...]; 404 when not indexed, 400 for a bad path. */
export interface FileCardPayload {
  v: 1;
  seq: number;
  kind: "file";
  card: FileCard;
  links: {
    view: string;
    incoming: { id: string; count: number }[];
    outgoing: { id: string; count: number }[];
  } | null;
}

/** GET /api/card?path=P&symbol=S */
export interface SymbolCardPayload {
  v: 1;
  seq: number;
  kind: "symbol";
  card: SymbolCard;
}

export type CardPayload = FileCardPayload | SymbolCardPayload;

/** GET /api/file?path=P; 404 not indexed, 400 bad path, 403 outside the project. */
export interface SourceFile {
  v: 1;
  path: string;
  lang: string | null;
  text: string;
  lines: number;
  truncated: boolean;
}

export interface SearchResult {
  kind: "file" | "symbol";
  path: string;
  symbol: string | null;
  label: string;
}

/** GET /api/search?q=Q: at most 20 results, files before symbols, then by label. */
export interface Search {
  v: 1;
  results: SearchResult[];
}

/** GET /api/journeys (empty until a later level fills it). */
export interface Journeys {
  v: 1;
  journeys: unknown[];
}

/** GET /api/tech (empty until a later level fills it). */
export interface Tech {
  v: 1;
  categories: unknown[];
}

/** The data of a `facts` event on GET /api/events. */
export interface FactsDelta {
  seq: number;
  at_ms: number;
  took_ms: number;
  files_changed: string[];
  files_removed: string[];
  regrouped: boolean;
  groups_changed: string[];
}
