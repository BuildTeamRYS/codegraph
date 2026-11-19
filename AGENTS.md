<!-- codegraph:start -->
This repo has a codegraph index (.codegraph/index.db). Use it before opening many files:
- Locate symbols, callers/callees and impact with the `codegraph` MCP tools (`find_symbol`, `outline`, `callers`, `callees`, `get_context`), or `codegraph query <tool>` from a shell.
- Read only the `file:start-end` ranges results return; edges carry `[certain|inferred|unknown]` confidence.
- If the header says `index@<commit> stale` or a result is `unknown`, fall back to normal search.
- After large refactors, run `codegraph index` to refresh the store.
<!-- codegraph:end -->
