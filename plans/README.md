# Plans

This directory preserves the reasoning behind ArrIR, not just a list of features.

- [Design contract and incremental roadmap](design.md): the agreed architecture,
  source mapping, boundaries and implementation sequence.
- [arrir-layout](chunked-layout.md): the storage-layout component, a Rust binding of
  the byterefs specification.
- [26 October 2026 discussion](2026-10-26-discussion.md): why the old scaffold is
  being replaced and the constraints Charles set for the rebuild.
- [27 October 2026 discussion](2026-10-27-discussion.md): reconciling #16 and #17 with
  this contract: purity rules, the dependency graph, `Open`, and the byterefs layout.

Update these notes when a decision changes, stating what changed and why.
Distinguish proposed designs from implemented behaviour. Passing type/unit tests
does not establish cross-host equivalence; adapter execution tests are required.
