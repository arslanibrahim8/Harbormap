-- prov joins the edge key: a parsed and an inferred edge with the same ends and line both stand.
CREATE TABLE edge_v2(kind TEXT NOT NULL, src_id TEXT NOT NULL, dst_id TEXT NOT NULL,
  file_id TEXT NOT NULL REFERENCES file(id) ON DELETE CASCADE, line INTEGER NOT NULL,
  rule TEXT NOT NULL, prov TEXT NOT NULL, conf TEXT NOT NULL,
  PRIMARY KEY(kind, src_id, dst_id, line, prov));
INSERT INTO edge_v2 SELECT kind, src_id, dst_id, file_id, line, rule, prov, conf FROM edge;
DROP TABLE edge;
ALTER TABLE edge_v2 RENAME TO edge;
CREATE INDEX edge_dst ON edge(dst_id, kind);
CREATE INDEX edge_file ON edge(file_id);
CREATE INDEX edge_prov ON edge(prov);
CREATE TABLE route(id TEXT PRIMARY KEY, root TEXT NOT NULL, entry_kind TEXT NOT NULL,
  method TEXT NOT NULL, pattern TEXT NOT NULL, handler_id TEXT,
  file_id TEXT NOT NULL REFERENCES file(id) ON DELETE CASCADE, line INTEGER NOT NULL,
  rule TEXT NOT NULL, prov TEXT NOT NULL, conf TEXT NOT NULL, detail TEXT);
CREATE INDEX route_file ON route(file_id);
CREATE INDEX route_handler ON route(handler_id);
CREATE TABLE client_call(file_id TEXT NOT NULL REFERENCES file(id) ON DELETE CASCADE,
  src_id TEXT NOT NULL, line INTEGER NOT NULL, method TEXT NOT NULL, url TEXT NOT NULL,
  rule TEXT NOT NULL, PRIMARY KEY(file_id, line, method, url, rule));
CREATE TABLE manifest(file_id TEXT PRIMARY KEY REFERENCES file(id) ON DELETE CASCADE,
  root TEXT NOT NULL, kind TEXT NOT NULL, name TEXT, lockfile TEXT, detail TEXT NOT NULL);
CREATE TABLE dependency(file_id TEXT NOT NULL REFERENCES file(id) ON DELETE CASCADE,
  section TEXT NOT NULL, name TEXT NOT NULL, declared TEXT NOT NULL, resolved TEXT,
  dev INTEGER NOT NULL, line INTEGER NOT NULL, PRIMARY KEY(file_id, section, name));
CREATE TABLE pattern(file_id TEXT NOT NULL REFERENCES file(id) ON DELETE CASCADE,
  subject_id TEXT NOT NULL, kind TEXT NOT NULL, line INTEGER NOT NULL, rule TEXT NOT NULL,
  conf TEXT NOT NULL, term TEXT, detail TEXT, PRIMARY KEY(file_id, subject_id, kind, line, rule));
CREATE TABLE gap(file_id TEXT NOT NULL REFERENCES file(id) ON DELETE CASCADE,
  subject_id TEXT NOT NULL, kind TEXT NOT NULL, line INTEGER NOT NULL,
  PRIMARY KEY(file_id, kind, line));
-- Facts gain web facts in L2a: every file is parsed again on the next pass.
UPDATE file SET facts_json = NULL;
