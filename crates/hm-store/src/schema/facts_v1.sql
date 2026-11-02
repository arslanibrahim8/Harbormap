CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE file(id TEXT PRIMARY KEY, path TEXT NOT NULL UNIQUE, lang TEXT, class TEXT NOT NULL,
  size INTEGER NOT NULL, mtime_ns INTEGER NOT NULL, loc INTEGER NOT NULL, content_hash TEXT NOT NULL,
  interface_hash TEXT, facts_hash TEXT, parse_status TEXT NOT NULL, facts_json TEXT);
CREATE TABLE symbol(id TEXT PRIMARY KEY, file_id TEXT NOT NULL REFERENCES file(id) ON DELETE CASCADE,
  kind TEXT NOT NULL, name TEXT NOT NULL, owner TEXT NOT NULL, sig TEXT NOT NULL, sig_hash TEXT NOT NULL,
  body_hash TEXT NOT NULL, l0 INTEGER NOT NULL, l1 INTEGER NOT NULL, exported INTEGER NOT NULL,
  lines INTEGER NOT NULL, branches INTEGER NOT NULL);
CREATE INDEX symbol_file ON symbol(file_id);
CREATE TABLE edge(kind TEXT NOT NULL, src_id TEXT NOT NULL, dst_id TEXT NOT NULL,
  file_id TEXT NOT NULL REFERENCES file(id) ON DELETE CASCADE, line INTEGER NOT NULL,
  rule TEXT NOT NULL, prov TEXT NOT NULL, conf TEXT NOT NULL, PRIMARY KEY(kind, src_id, dst_id, line));
CREATE INDEX edge_dst ON edge(dst_id, kind);
CREATE INDEX edge_file ON edge(file_id);
CREATE TABLE ref(file_id TEXT NOT NULL REFERENCES file(id) ON DELETE CASCADE, kind TEXT NOT NULL,
  text TEXT NOT NULL, line INTEGER NOT NULL, status TEXT NOT NULL,
  PRIMARY KEY(file_id, kind, text, line));
CREATE TABLE reverse_import(dst_file_id TEXT NOT NULL, src_file_id TEXT NOT NULL
  REFERENCES file(id) ON DELETE CASCADE, PRIMARY KEY(dst_file_id, src_file_id));
