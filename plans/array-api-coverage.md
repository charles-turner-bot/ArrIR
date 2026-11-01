# Array API 2025.12 coverage

Status: audit, 1 November 2026, for #41. Its recommendations were accepted on
1 November (see [Decisions](#decisions)). This maps the Array API onto ArrIR's
designed operations. It doesn't claim conformance and doesn't ask for one node
per function.

**Pinned source:** <https://data-apis.org/array-api/2025.12/>, retrieved
2026-11-01. Function lists were read from the category pages under
`API_specification/` and `extensions/` at that path, not from memory. Every page fetched.

## Implemented versus proposed

The replacement IR implements nothing yet. The legacy scaffold (`src/lib.rs`, to become
`arrir::legacy` in #15) has `Select`, `Rechunk` and a `Reduce` with an open string name,
which [design.md](design.md#what-to-extract) rejects ("name alone is insufficient").
Every class below describes *proposed* representation.

## Classes

Each function has exactly one class. A condition that would change it is a footnote.

| Class | Meaning |
| --- | --- |
| **Direct** | A designed or proposed operation represents it. |
| **Compose** | A composition of designed or proposed operations represents it, with no loss of meaning. |
| **Compose after New** | A composition that needs at least one operation classed New. |
| **New** | Needs an operation that no design or issue proposes yet. |
| **Blocked** | A proposed operation would represent it, but a contract it depends on is undecided. |
| **Meta** | Answered by lowering from declared metadata; no node. |
| **Runtime** | The result or its shape depends on data. Needs symbolic extents or a host barrier. |
| **Deferred** | Out of scope until a data-access use case needs it. |

shapes.md gives every axis a name or none: a shape is all `Named(DimId)` or all
`Positional`. Array API arrays lower to positional shapes, so an `axis` argument
addresses a position directly and no `DimId` is invented. Normalising a negative axis
is **Meta**.

Literal position lists ([#3](https://github.com/charles-turner-bot/arrir/issues/3))
need concrete lengths. Compositions built from them (`roll`, `tile`, `repeat`) reject a
symbolic extent ([shapes.md](shapes.md)).

## Semantic rules

| Rule | Class | Representation and evidence |
| --- | --- | --- |
| Broadcasting | Meta | Shape inference in the core, shapes.md (for #29), with no `Broadcast` node. Decided 1 November: positional shapes follow the Array API in full; named shapes align by name, broadcast a dimension that only one operand has, and never stretch a length-1 axis; a positional operand adopts a named operand's names. #34 keeps only coordinate label alignment. |
| Type promotion, scalar arguments | Meta | Decided in [dtypes.md](dtypes.md) (for #30); to be implemented in #31. Mixed kinds are rejected (`MixedKind`, its question 1, decided 1 November). Python scalars become `Constant` nodes. |
| Data types | Meta | Bool, int8–64, uint8–64, float32/64 are in #26's `DType`. complex64/128 are **required** by the spec but deferred in #26 and dtypes.md. |
| Basic indexing (int, slice, ellipsis) | Direct | `Select` positional indexers, #3; conventions in [design.md](design.md#conventions). |
| Integer-array indexing | Direct | The spec's subset of vectorised indexing; #36 compares it with orthogonal indexing and chooses the modes. Slices mixed with integer arrays are unspecified by the spec; reject. |
| Boolean-array indexing | Runtime | #36; the extent is a symbol for the true count (a `CountNonZero` size term in #36 and shapes.md). A literal mask has a known count (#3). |
| Data-dependent output shapes | Runtime | Symbolic extents (#29/shapes.md, #36). The spec lets libraries omit these; adapters advertise this as a capability. |
| Copies, views, mutation | Deferred | Plans are values, and the proposed node arena is append-only (dag.md). `copy=` and in-place operators are adapter concerns. |
| Device support | Deferred | Outside the logical core, as #41 requires; no result semantics depend on it. |
| Lazy versus eager | Meta | ArrIR is lazy by construction. Calls that force a value (`__bool__` and so on) are host barriers. |
| Accuracy | Meta | The spec doesn't fix transcendental accuracy. ArrIR keeps evaluation order; fixtures state tolerances (#42). |

## Functions

Each function appears once. Proposed representation is in the third column.

| Functions | Class | Representation and evidence |
| --- | --- | --- |
| **Creation** | | |
| `asarray` | Direct | An `Open` with a binding, or for a scalar, dtypes.md's `Constant { value, nan_is_missing }` (its question 6, decided). Embedded arrays are not allowed ([design.md](design.md#decisions)). |
| `from_dlpack` | Deferred | Host interchange, outside the core. |
| `zeros`, `ones`, `full`, `zeros_like`, `ones_like`, `full_like` | New | A constant with a declared shape. dtypes.md's `Constant` is zero-dimensional and broadcasts inside elementwise operations, so a shaped fill is needed only when materialised on its own. |
| `empty`, `empty_like` | Deferred | Uninitialised values have no defined meaning. |
| `arange`, `linspace` | New | A generated index sequence. Low priority: regular grids are declared ([grids.md](grids.md)). |
| `meshgrid` | Compose after New | Positional `expand_dims` on each input, then `broadcast_arrays`, which is also Blocked (below). The default `indexing='xy'` also swaps the first two dimensions (Transpose, #4). |
| `eye`, `tril`, `triu` | Deferred | Compose from `arange`, comparison and `where` once those exist; no data-access use. |
| **Data types** | | |
| `astype` | Direct | `Cast`, [cast.md](cast.md), #38. Stricter than the spec: rounding and out-of-range policies are explicit where the spec leaves them implementation-defined. |
| `can_cast`, `result_type`, `isdtype`, `finfo`, `iinfo`, dtype `__eq__` | Meta | Promotion table in dtypes.md. |
| **Elementwise**, specified in [elementwise.md](elementwise.md) (#32) except where a row says otherwise | | |
| `add`, `multiply`, `greater` | Direct | First `Elementwise` variants, #32; to be implemented in #33. |
| `subtract`, `negative`, `abs`, `square`, `pow`, `floor_divide`, `remainder`, `bitwise_left_shift`, `bitwise_right_shift` | Direct | #32 variants. Integers wrap; `pow`, `floor_divide` and `remainder` are integer-only for now (float deferred). Shifts still need shift-count rules. |
| `divide`, `sqrt`, `reciprocal`, `maximum`, `minimum`, `hypot`, `copysign`, `nextafter`, `logaddexp`, `sign` | Direct | #32 variants. `divide`, `maximum`, `minimum` and `sign` are specified; `sqrt`, `reciprocal`, `hypot`, `copysign`, `nextafter` and `logaddexp` still need NaN, infinity and signed-zero rules. |
| `exp`, `expm1`, `log`, `log1p`, `log2`, `log10`, `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `sinh`, `cosh`, `tanh`, `asinh`, `acosh`, `atanh` | Direct | #32 variants. Need an accuracy contract, written as a named permission (dtypes.md decision 5); bitwise equality across engines isn't promised. |
| `equal`, `not_equal`, `greater_equal`, `less`, `less_equal`, `logical_and`, `logical_or`, `logical_xor`, `logical_not`, `bitwise_and`, `bitwise_or`, `bitwise_xor`, `bitwise_invert`, `positive`, `ceil`, `floor`, `trunc`, `round`, `isnan`, `isinf`, `isfinite`, `signbit` | Direct | #32 variants. Exact; specified except the bitwise functions and `positive`. `round` ties to even, distinct from `Cast`'s `Rounding`. `isnan` is not "is missing" (#39). |
| `clip` | Compose | `maximum` then `minimum`; scalar promotion as clarified in 2025.12 (dtypes.md). |
| `conj`, `real`, `imag` | Deferred | Complex dtypes (#26, dtypes.md). |
| **Indexing** | | |
| `take` | Direct | `Select` with a bound position array on one dimension, #36. |
| `take_along_axis` | New | Not in #36's current scope.¹ |
| **Linear algebra** | | |
| `matrix_transpose` | Direct | Transpose of the last two dimensions, #4. |
| `vecdot` | Compose | `multiply` then `Reduce(sum)`, which declares its `order` and `result` dtype (dtypes.md). Lowering from NumPy writes `order: Unspecified`, as the spec promises no order. |
| `matmul`, `tensordot` | Deferred | Semantically `expand_dims` + `multiply` + `sum`, but with an O(n·m·k) intermediate. A contraction primitive waits for a use case such as regridding weights; `Warp` covers regridding today. |
| **Manipulation** | | |
| `broadcast_shapes` | Meta | #29, shapes.md. |
| `broadcast_to`, `broadcast_arrays` | Deferred | shapes.md decided on no `Broadcast` node (question 7), so only the absorbed case is supported; every other use, including at the plan root, is rejected until a use case justifies a shape-only node.⁵ |
| `concat` | New | Variadic (`AtLeast(1)` in dag.md), extent sum along a dimension, promotion across inputs. No design. With symbolic extents it also needs a sum size term, which shapes.md's `SizeTerm` doesn't define. |
| `stack` | Compose after New | `expand_dims` on each input, then `concat`. Both are New; the broadcasting decision doesn't change that. |
| `expand_dims` | New | Inserts a positional length-1 axis. Also `newaxis`. Positional only: named shapes broadcast absent dimensions themselves (shapes.md), so they don't need it. |
| `squeeze` | Compose | `Select` with scalar index 0, #3. Only when the extent is proven to be 1; a symbolic extent is rejected. |
| `flip` | Compose | `Select` slice with step −1, #3. |
| `permute_dims`, `moveaxis` | Direct | Transpose, #4. Negative axes (2025.12) resolve at lowering. |
| `reshape` | Blocked | `Reshape`, #5, but the spec reshapes in row-major order and #5 checks only element counts. Blocked on a traversal-order contract. |
| `repeat` | Compose | `Select` with a literal position list (#3 allows repeats).² |
| `roll` | Compose | Literal position list, #3. |
| `tile` | Compose | Literal position lists, #3.³ |
| `unstack` | Compose | One scalar `Select` per output; one root each (dag.md). |
| **Searching** | | |
| `argmax`, `argmin`, `count_nonzero` | Direct | `Reduce` variants (roadmap step 5). Lowering states the index or counter dtype, as dtypes.md does for `count_nonzero`. |
| `nonzero` | Deferred | Its extent is Runtime (`CountNonZero`, shapes.md) and its order row-major. Boolean indexing (#36) covers the use case. |
| `searchsorted` | Deferred | Known output shape, but needs a sortedness precondition planning can't check. Label resolution, its main use, is outside the core. |
| `where` | Direct | `Where`, a ternary `Elementwise` variant ([elementwise.md](elementwise.md); decided 1 November, see [Decisions](#decisions)). |
| **Sets** | | |
| `unique_all`, `unique_counts`, `unique_inverse`, `unique_values` | Deferred | Extents are Runtime; no data-access use yet. |
| `isin` | Deferred | Known shape, but no data-access use yet. |
| **Sorting** | | |
| `sort`, `argsort` | Deferred | Shape-preserving, but NaN ordering and stability need a contract. |
| **Statistical** | | |
| `sum`, `prod`, `mean`, `min`, `max` | Direct | `Reduce`, roadmap step 5. dtypes.md: `sum`, `prod` and `mean` declare an accumulator, a separate `result` dtype and an `order` (`Sequential` or `Unspecified`, no default); `min`/`max` keep the input dtype. To be implemented in #31. |
| `std`, `var` | Blocked | dtypes.md leaves them out until their formula (two-pass, Welford or other) has a contract. `correction` (xrexpr's `ddof`) would be a result-affecting option. |
| `cumulative_sum`, `cumulative_prod` | Blocked | `Scan` ([design.md](design.md#what-to-extract)), which has no issue or design yet. `include_initial` adds one to the extent; on a symbolic extent that needs an offset size term shapes.md doesn't define. |
| **Utility** | | |
| `all`, `any` | Direct | Boolean `Reduce` variants. |
| `diff` | Compose | `subtract` of two shifted slices (#3, #32).⁴ |
| **Constants** | | |
| `e`, `pi`, `inf`, `nan` | Direct | `Constant` nodes (dtypes.md), typed by the operand they meet. `nan` sets `nan_is_missing: false` from Array API lowering. |
| `newaxis` | New | Same as `expand_dims`. |
| **Inspection** | | |
| `__array_namespace_info__`, `capabilities`, `default_device`, `default_dtypes`, `devices`, `dtypes` | Meta | Host side. `default_dtypes` affects results, so lowering states dtypes explicitly (dtypes.md). `capabilities` (boolean indexing, data-dependent shapes, max rank) maps to adapter acceptance. |
| **Array object** | | |
| `dtype`, `shape`, `ndim`, `size`, `device` | Meta | Schema facts (#26). `shape` may contain unknown extents. |
| `T`, `mT` | Direct | Transpose, #4. |
| Operators (`__add__`, `__lt__`, `__matmul__`, …) | — | Same as their functions above. |
| `__getitem__` | Direct | Indexing rules above. |
| `__setitem__`, in-place operators | Deferred | Mutation. Assigning through a full Boolean mask is `where`; indexed assignment is a scatter, which nothing proposes. |
| `__bool__`, `__int__`, `__float__`, `__complex__`, `__index__` | Runtime | Force a value: host barrier. |
| `__array_namespace__`, `__dlpack__`, `__dlpack_device__`, `to_device` | Deferred | Host interchange and device. |
| **Extensions** | | |
| `linalg.*` (25 functions: `cholesky`, `cross`, `det`, `diagonal`, `eig`, `eigh`, `eigvals`, `eigvalsh`, `inv`, `matmul`, `matrix_norm`, `matrix_power`, `matrix_rank`, `matrix_transpose`, `outer`, `pinv`, `qr`, `slogdet`, `solve`, `svd`, `svdvals`, `tensordot`, `trace`, `vecdot`, `vector_norm`) | Deferred | Optional extension. The aliases follow their main-namespace rows. |
| `fft.*` (`fft`, `ifft`, `fftn`, `ifftn`, `rfft`, `irfft`, `rfftn`, `irfftn`, `hfft`, `ihfft`, `fftfreq`, `rfftfreq`, `fftshift`, `ifftshift`) | Deferred | Optional extension. `fftshift` would compose as `roll`. |

1. Becomes Direct if #36 adopts a vectorised mode that covers it.
2. Array-valued `repeats` is Runtime. `axis=None` flattens first, so it depends on
   `reshape` and is Blocked.
3. Repetitions longer than the rank add leading dimensions and need `expand_dims` (New).
4. `prepend` and `append` need `concat` (New).
5. Meta, with no node, only when the consumer's output shape already covers the
   target shape: `broadcast_to(x(3,), (2, 3)) + y(2, 3)` can drop the call, but
   `broadcast_to(x(3,), (2, 3)) + y(3,)` would give `(3,)` instead of `(2, 3)`. A
   broadcast at the root, or one consumed by a reduction, always needs the node.

## Counts

The 135 main-namespace functions. The counts leave out the 6 inspection functions,
the constants, operators, array attributes and the two extensions.

| Class | Count |
| --- | --- |
| Direct | 80 |
| Compose | 9 |
| Compose after New | 2 |
| New | 11 |
| Blocked | 5 |
| Meta | 6 |
| Runtime | 0 |
| Deferred | 22 |
| **Total** | **135** |

Runtime applies to rules and array methods, not to any counted function: the
data-dependent functions (`nonzero`, `unique_*`) are Deferred. 64 Direct rows are
`Elementwise` variants. [elementwise.md](elementwise.md) specifies 32 of them; the
rest each need the rules named in their group.

## Missing concepts, by data-access use case

Only concepts that no design or issue yet covers. Ranked by need in selection,
reduction, alignment and regridding, not by catalogue size.

1. **`concat`** (alignment). Assumes datasets split across files along time reach
   ArrIR as several `Open`s. If the byterefs layout ([chunked-layout.md](chunked-layout.md))
   presents them as one `Open`, this drops below the `Reduce` issue. Needs a sum size
   term in shapes.md for symbolic extents.
2. **A `Reduce` specification issue** (reduction). Roadmap step 5 exists, and dtypes.md
   covers accumulators, `result` and `order`. `argmax`/`argmin`, the `var`/`std`
   formula and `correction`, boolean `all`/`any` and `count_nonzero` have no ticket.
3. **`expand_dims`** (positional only). Named shapes no longer need it (shapes.md), so
   it serves NumPy-style code: `x[:, None]`, `stack`, `meshgrid` and `tile`.
4. **A traversal-order contract** (selection). `reshape`, `nonzero` and boolean indexing
   all yield row-major order. #5 needs it before claiming equivalence, and #36 needs it
   for masks over more than one dimension.
5. **`Scan`** (reduction). In the extraction table, with no issue. `include_initial`
   needs an offset size term in shapes.md.

Deferred until a use case arrives: shaped constants, contractions (`matmul`,
`tensordot`), sort and set operations, `searchsorted`, the `linalg` and `fft`
extensions, and complex dtypes.

## Decisions

Charles accepted each recommendation below on 1 November 2026.

1. **`broadcast_to`** (with shapes.md's question 7). Decided: lowering drops the
   call only when the consumer's output shape already covers the target, and rejects
   every other case, the plan root included. shapes.md decided against a `Broadcast`
   node for now. Multiplying by a `ones` constant was considered and rejected:
   missingness depends on #39, a `ones` can't be typed when the dtype is Unknown, and a
   rewrite folding `x * 1` to `x` would drop the broadcast.
2. **`where`** (#32). A ternary `Elementwise` variant or its own node? Decided: a
   variant. It shares broadcasting and promotion inference with the rest of the family;
   its condition must be Bool.
3. **`concat` first?** Decided: yes, if multi-file time series reach ArrIR as
   several `Open`s. Confirm against how the byterefs layout presents them.
4. **Complex dtypes.** The spec requires them; #26 and dtypes.md defer them and reject at
   lowering. Decided: keep them deferred until a data-access use case (spectra,
   for example). Rejecting them explicitly is honest about the gap.

## Notes for other documents

- [design.md](design.md#conventions) now says no operation reorders dimensions except
  as its documented output-order rule says. Transpose (#4) should add its rule there
  when it lands.
