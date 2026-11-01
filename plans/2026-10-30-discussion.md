# 30 October: start building, with explicit schema extension points

Charles requested bringing #21 onto main, then schema design thoughts using enums
with variants wherever concepts will need generalising. The intention is to extend
the algebra without replacing its enclosing types, not to implement every variant now.

- #21 had merged into `design/grids`, after that branch landed on main. Integration
  PR #25 brings its original history and Select-on-grids rules onto main.
- [schema-and-values.md](schema-and-values.md) proposes result kinds, numeric value
  types, coordinate roles and the distinction between unknown and absent facts.
- It reuses the existing grid/missingness designs, and clarifies the placement rule
  so #21's explicit removal of a grid is a valid schema state.
- The first code slice stays small: IDs/workspace, schema, Open/Select/one Reduce,
  then an adapter execution proof. No competing scaffold or speculative executor.
- This is a review proposal, not an implemented or already-agreed schema API.

Source check: xrexpr `3597a2e` (`schema.py`, `ir.py`, `lower.py`); these files
match the locally fetched `origin/main`. This supports the variable-first schema
and replay-option boundary, and does not change the pinned extraction revision.
