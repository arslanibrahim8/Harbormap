# Explanations for Harbormap: deterministic facts first, optional LLM prose

Scope note: search results were mostly secondary sources. Items marked (unverified) come from one blog or a project's own claim. I did not find solid, current primary data on Swimm, CodeSee, Continue, GitNexus or gitdiagram internals, so those are left out of the table rather than guessed.

## 1. How existing tools explain code, and at what cost

- **DeepWiki (Devin).** Cognition's docs say Devin indexes repos and produces wikis with architecture diagrams, source links and summaries; the same index powers Ask Devin. Public repos are free. Steered wikis are capped at 30 pages (80 on enterprise), per third-party guides. No official token cost is published. https://docs.devin.ai/work-with-devin/deepwiki , https://docs.devin.ai/onboard-devin/index-repo , https://fast.io/resources/devin-deepwiki-guide/
- **Google Code Wiki.** Rescans the whole repo and regenerates docs after each change. A Gemini chat uses the wiki as its knowledge base. Every section and answer links to files and definitions. Public GitHub repos only, in preview. https://developers.googleblog.com/introducing-code-wiki-accelerating-your-code-understanding/ , https://devops.com/google-code-wiki-aims-to-solve-documentations-oldest-problem/
- **Understand-Anything.** Static analysis builds a skeleton (files, functions, classes, imports, calls). Multi-agent LLMs then add semantics. The README warns the first run can use many tokens. One guide reports about 200k tokens and about 30 minutes on the small Online Boutique demo, with incremental reruns afterwards. That is roughly the "burns tokens" complaint you cite. https://github.com/Egonex-AI/Understand-Anything , https://betterstack.com/community/guides/ai/understand-anything/ (owner name differs across sources; check which repo is canonical)
- **codebase-memory-mcp.** Tree-sitter into a local SQLite graph (functions, classes, call chains, routes), queried by agents. Claims about 99% fewer tokens (412k vs 3.4k for five structural queries). It is the project's own benchmark, and it covers structural questions, not prose explanation. (unverified) https://dev.to/andrew-ooo/codebase-memory-mcp-review-99-fewer-tokens-for-ai-agents-4789
- **aider repo-map.** No LLM at map time. Tree-sitter extracts definitions and references, personalized PageRank ranks them, and the top ones are rendered within a token budget (default 1,024, found by binary search). This is the best model of "deterministic context, tiny budget". https://www.agentpatterns.ai/context-engineering/repository-map-pattern/ , https://fast.io/resources/aider-token-limit/
- **Cursor indexing.** A Merkle tree of file and folder hashes finds what changed. AST-aware chunks are embedded, and embeddings are cached by chunk content hash. This is the template for Harbormap's cache. https://cursor.com/blog/secure-codebase-indexing , https://read.engineerscodex.com/p/how-cursor-indexes-codebases-fast
- **Sourcegraph Cody / Greptile.** Retrieval over a code graph (definitions, references, symbols, doc comments) plus keyword search. Neither source describes per-function LLM summaries. https://sourcegraph.com/docs/cody/core-concepts/code-graph , https://greptile.com/blog/seed
- **Hierarchical summarization research.** Cisco's Code-Craft builds a code graph and summarizes bottom-up, so higher-level summaries inherit lower ones. It reports up to 82% relative top-1 precision gain over code-only retrieval. https://www.arxiv.org/pdf/2504.08975

Takeaways:
- Wiki-style tools (DeepWiki, Code Wiki, Understand-Anything) spend LLM tokens on everything. The leaner tools (aider, Cursor, codebase-memory) do the structural work deterministically and use content hashes for invalidation.
- No tool publishes a clean "tokens per 1k files" number. Estimate it yourself (section 4).

## 2. Deterministic pattern and method detection

What works without an LLM, ordered by reliability:
1. **Syntax facts (tree-sitter).** Functions, classes, signatures, decorators or attributes, imports, exported symbols, call sites by name. Tree-sitter queries are precise for syntax. Name-based call resolution is heuristic.
2. **Framework signatures.** Decorators and attributes are close to ground truth: `@router.post`, `@app.get`, `@Injectable()`, `@Controller`, `[ApiController]`, `@Service`, `@Repository`, `@Scheduled`, `@celery.task`, `@retry`, `@Transactional`, `#[tokio::main]`, `axum::Router`. Add manifest evidence (package.json, pyproject, Cargo.toml, pom.xml, *.csproj dependencies) to tell which framework is in play.
3. **Import and layer graph.** Controller importing service importing repository gives layering. Cycles, fan-in and fan-out, and "domain imports infrastructure" are all computable.
4. **Naming and path conventions.** `*Repository`, `*Service`, `*Handler`, `*Command`, `*Query`, `*Worker`, `migrations/`, `jobs/`. These are weak on their own and become medium confidence when confirmed by structure (a Repository class that touches a DB client).
5. **Library calls as pattern evidence.** Retry (`tenacity`, `backoff`, Polly, `resilience4j`), queues (`celery`, `bull`, `sidekiq`), DI containers (`inject`, `dependency_injector`, Spring, Nest modules), CQRS (command/query handler pairs, MediatR).

Rule engines: Semgrep uses a single YAML schema across languages with metavariables, ellipsis and `pattern-inside`, which is easy for users to extend. Its limits are cross-file and interprocedural analysis. For Harbormap, tree-sitter queries embedded in the Rust binary are lighter, and Semgrep-style YAML can be the user-facing rule format. https://docs.semgrep.dev/for-developers/detection.md (limits are from community guides, not benchmarks)

Accuracy limits: I found no published precision/recall for architectural pattern detectors, so measure it yourself on a labelled set of repos. Expect high precision for explicit decorators and low recall for hand-rolled patterns (manual DI, custom retry loops). Abstract patterns such as "hexagonal" or "CQRS without a library" cannot be proven from syntax. Report them as hints.

"Does the structure fit its job" can be done partly deterministically with rules: file size and function count, a controller containing DB calls, a repository with business branching, a cycle, a god-class fan-out. Show these as findings with numbers, never as a verdict.

## 3. Grounding and trust

- Every fact carries `file:line-range`, a rule id, and a confidence label: **Certain** (explicit decorator, manifest), **Likely** (naming plus structural confirmation), **Hint** (naming only or LLM inference). Never render a Hint as a fact.
- Relations come only from the graph (resolved imports or calls). The LLM may never add an edge.
- Citation checking matters. In one study of 30 Python repos, models' citation compliance ranged 62-88%, and some cited line ranges that did not exist. The fix was a mechanical check of citations against file contents. https://arxiv.org/abs/2512.12117v1 (single preprint)
- Code summaries do hallucinate even from provided code; an FSE 2026 paper builds a dataset and detector for it. https://conf.researchr.org/details/fse-2026/fse-2026-research-papers/66/Hallucinations-in-LLM-based-Code-Summarization-Unveiling-Detection-and-Mitigation
- Harbormap design: "facts in, prose out". The prompt contains the fact list with ids. The model must cite fact ids, and the output is JSON. Harbormap validates it: unknown ids, unknown symbol names or numbers not in the facts cause the sentence to be dropped or the output to fall back to the deterministic template. Label the result "AI-written from verified facts" and keep the fact panel visible beside it.

## 4. Cost control

Prices (Anthropic): Batch API 50% off input and output; cache reads 0.1x, 5-minute cache writes 1.25x. Haiku 4.5 is about $1 in / $5 out per million tokens (third-party figure; check the live page). https://docs.anthropic.com/en/docs/about-claude/pricing , https://www.morphllm.com/anthropic-claude-api-pricing

Budget math for a 1,000-file repo (my estimates, not measured):
- Per-file card: about 350 input tokens (facts) plus 100 to 150 output tokens. Do not send source by default; send the signature list, decorators, imports and a few short snippets only when asked.
- File level for all 1,000 files: about 0.4M input and 0.15M output. On a cheap tier at Haiku 4.5 prices that is about $0.4 + $0.75, around $1.2, or about $0.6 with batch.
- Folder and service level (about 100 nodes, each fed the child summaries, about 1.5k in and 250 out): about 0.15M in. Under $0.20.
- Overview (one call, about 4k in and 800 out): a few cents on a better model.
- Default cap: 1,500 tokens per file call; about 600k tokens total for 1k files; and do on-click explanations only (zero spend until the user clicks). Compare with about 200k tokens for a small demo repo in Understand-Anything.
- Local models (Ollama or llama.cpp): no controlled summarization benchmark found. Practitioner sources say local models are fine for routine code explanation and weaker for multi-file reasoning; 7B models are frustrating for agentic work, so keep local prompts single-file and structured. Recommend a small built-in eval (20 files, rubric) before trusting a given local model. https://alexcloudstar.com/blog/local-ai-models-coding-ollama-2026/ , https://toolhalla.ai/blog/best-local-llms-for-coding-2026

Cost preview: count tokens before running using the actual prompts (a local tokenizer estimate, or a count-tokens endpoint), subtract cache hits, then show "N files miss cache, X input + Y output tokens, about $Z on model M, batch -50%". Require a click to confirm, and enforce a hard stop at the confirmed number.

## 5. Beginner-friendly writing

No single authoritative source found; this is design guidance. Use three levels, switchable per card:
- **Beginner:** what it is in one sentence, one analogy, no jargon, and no pattern names without a gloss. Example: "This file is the front desk of the Orders service. It receives web requests and hands them to the people who do the work."
- **Builder:** responsibilities, key functions with one line each, patterns found, what calls it and what it calls.
- **Expert:** facts table, evidence lines, findings (smells, cycles), confidence labels.

Project overview as a story, in five beats: what it is for, who uses it, the journey of one request or job through the parts, the main parts as places on a map (harbour, docks, warehouse), and what to read first. Fill it deterministically from entry points, routes, the top-ranked files and the layer graph, then optionally polish the prose with an LLM.

Length rules: card at most 60 words for beginner; max 5 "main functions", ranked by fan-in plus being exported or a route; always show "why we think so" evidence.

## Recommendation: what Harbormap should do, in order

1. **Deterministic fact layer (free, ships first).** Tree-sitter extraction, import and call graph, framework and manifest detection, pattern rules with evidence and confidence, layer and fit findings, and an aider-style PageRank ranking for "main files". Add template prose per level (see below) so the product is useful with zero tokens.
2. **Template explanations.** For each file: "<Role> in <service>. Uses: <patterns>. Main functions: ... Called by N, calls M." Generated from the facts with fixed sentences. This alone answers "what is it, what patterns, main functions".
3. **Evidence UI.** Every claim links to file:line with a confidence badge; a user can toggle rule ids.
4. **Content-hash cache.** Store SQLite rows keyed by `hash(file content) + hash(facts JSON) + prompt_version + model_id + level`. Folder and service keys hash the sorted child keys (a Merkle tree like Cursor's), so one edited file invalidates only its ancestors. A renamed or moved file with identical content still hits.
5. **Opt-in "Explain with AI" per click.** Prompt shape: system rules (use only provided facts; cite fact ids; no new relations; say "unknown" if absent), then a fact block (id, kind, name, signature, file:line, confidence), at most 40 lines of snippets for the top functions, then a JSON schema (`summary`, `analogy`, `pattern_notes[]` each with `fact_ids`, `fit_comment`). Validate citations mechanically; drop failures.
6. **Bottom-up hierarchy, only on request.** File to folder to service to system. Parents receive child summaries plus graph facts, never raw source. This is the Code-Craft shape and keeps parents small.
7. **Cost preview and budget.** Preview as in section 4, with cache hits shown as free. Defaults: Haiku-class model for file and folder, a stronger model only for the overview, batch mode for "explain everything", on-click as the default, 600k-token total cap per 1k files, and no source upload without an explicit toggle (show which files leave the machine).
8. **Local model support.** Ollama or llama.cpp via an OpenAI-compatible endpoint; same prompt, same validation; run the small eval before enabling "explain all".
9. **Measure.** Build a small labelled corpus (10 repos) to track pattern precision and recall, and the share of AI sentences dropped by validation. Publish the numbers.

## Comparison table

| Tool | Core method | LLM spend | Caching / invalidation | Evidence links | Fits Harbormap as |
|---|---|---|---|---|---|
| DeepWiki | LLM-written wiki + diagrams over index | Not published; free for public repos | Re-index (details unpublished) | Source links | Output-shape reference |
| Code Wiki | Full rescan, regenerate after change | Not published | Regenerate per change | Links to files/definitions | Cautionary on full regeneration |
| Understand-Anything | Static skeleton + multi-agent LLM | High (about 200k tokens, small demo, secondary source) | Incremental by changed files | Graph nodes | The cost to beat |
| codebase-memory-mcp | Tree-sitter graph in SQLite | None for indexing | Local, reindex on change | Graph queries | Fact layer reference |
| aider repo-map | Tree-sitter + PageRank | None | Per run | Code excerpts | "Main files" ranking |
| Cursor | AST chunks + embeddings | Embedding cost only | Merkle tree + chunk-hash cache | Line ranges | Cache design |
| Cody / Greptile | Code graph retrieval | At query time | Index updates | Code refs | Retrieval, not explanation |
| **Harbormap (proposed)** | Facts first, LLM optional | Zero by default; capped, previewed | Content-hash Merkle keys | file:line + confidence on every claim | n/a |
