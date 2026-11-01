# Array API 2025.12 coverage

Status: proposal, 1 November 2026. Planning for #41. This maps the Array API onto
ArrIR's proposed operations. It doesn't claim conformance and doesn't ask for one node
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

Array API axes are positions. ArrIR dimensions are named and ordered. Lowering maps an
axis (negative ones included) to a `DimId` from the input schema: that mapping is
**Meta** for every function with an `axis` argument.

Literal position lists ([#3](https://github.com/charles-turner-bot/arrir/issues/3))
need concrete lengths. Compositions built from them (`roll`, `tile`, `repeat`) reject a
symbolic extent ([shapes.md](https://github.com/charles-turner-bot/arrir/pull/46), PR #46).

## Semantic rules

| Rule | Class | Representation and evidence |
| --- | --- | --- |
| Broadcasting | Meta | Positional shape inference, #29, proposed in PR #46 with no `Broadcast` node. Whether aligned axes must share a `DimId` (`AxisNameMismatch`) is #46's open decision A; it recommends the strict check, with Array API lowering inserting `Rename`. Named broadcasting is separate (#34). |
| Type promotion, scalar arguments | Meta | Proposed in PR #47 (dtypes.md, for #30); to be implemented in #31. Rejecting mixed kinds (`MixedKind`) is its open question 1. Python scalars become typed constants. |
| Data types | Meta | Bool, int8–64, uint8–64, float32/64 are in #26's `DType`. complex64/128 are **required** by the spec but deferred in #26 and PR #47. |
| Basic indexing (int, slice, ellipsis) | Direct | `Select` positional indexers, #3; conventions in [design.md](design.md#conventions). |
| Integer-array indexing | Direct | The spec's subset of vectorised indexing; #36 compares it with orthogonal indexing and chooses the modes. Slices mixed with integer arrays are unspecified by the spec; reject. |
| Boolean-array indexing | Runtime | #36; the extent is a symbol for the true count (a `CountNonZero` size term in #36 and PR #46). A literal mask has a known count (#3). |
| Data-dependent output shapes | Runtime | Symbolic extents (#29/PR #46, #36). The spec lets libraries omit these; adapters advertise this as a capability. |
| Copies, views, mutation | Deferred | Plans are values, and the proposed node arena is append-only (PR #45). `copy=` and in-place operators are adapter concerns. |
| Device support | Deferred | Outside the logical core, as #41 requires; no result semantics depend on it. |
| Lazy versus eager | Meta | ArrIR is lazy by construction. Calls that force a value (`__bool__` and so on) are host barriers. |
| Accuracy | Meta | The spec doesn't fix transcendental accuracy. ArrIR keeps evaluation order; fixtures state tolerances (#42). |

## Functions

Each function appears once. Proposed representation is in the third column.

| Functions | Class | Representation and evidence |
| --- | --- | --- |
| **Creation** | | |
| `asarray` | Direct | An `Open` with a binding, or a typed scalar constant (PR #47's proposed `Constant(Scalar)`, its open question 6). Embedded arrays are not allowed ([design.md](design.md#decisions)). |
| `from_dlpack` | Deferred | Host interchange, outside the core. |
| `zeros`, `ones`, `full`, `zeros_like`, `ones_like`, `full_like` | New | A constant with a declared shape. PR #47 proposes only a zero-dimensional constant; giving it a shape needs a broadcast (#46's question 7). |
| `empty`, `empty_like` | Deferred | Uninitialised values have no defined meaning. |
| `arange`, `linspace` | New | A generated index sequence. Low priority: regular grids are declared ([grids.md](grids.md)). |
| `meshgrid` | Compose after New | `expand_dims` on each input, then a broadcast (#46, question 7). The default `indexing='xy'` also swaps the first two dimensions (Transpose, #4). |
| `eye`, `tril`, `triu` | Deferred | Compose from `arange`, comparison and `where` once those exist; no data-access use. |
| **Data types** | | |
| `astype` | Direct | `Cast`, [cast.md](cast.md), #38. Stricter than the spec: rounding and out-of-range policies are explicit where the spec leaves them implementation-defined. |
| `can_cast`, `result_type`, `isdtype`, `finfo`, `iinfo`, dtype `__eq__` | Meta | Promotion table in PR #47 (proposed). |
| **Elementwise**, grouped by what #32 still has to state | | |
| `add`, `multiply`, `greater` | Direct | First `Elementwise` variants, #32; to be implemented in #33. |
| `subtract`, `negative`, `abs`, `square`, `pow`, `floor_divide`, `remainder`, `bitwise_left_shift`, `bitwise_right_shift` | Direct | #32 variants. Need integer overflow, division-by-zero and shift-count rules. |
| `divide`, `sqrt`, `reciprocal`, `maximum`, `minimum`, `hypot`, `copysign`, `nextafter`, `logaddexp`, `sign` | Direct | #32 variants. Need NaN, infinity and signed-zero rules. |
| `exp`, `expm1`, `log`, `log1p`, `log2`, `log10`, `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `sinh`, `cosh`, `tanh`, `asinh`, `acosh`, `atanh` | Direct | #32 variants. Need stated tolerances; bitwise equality across engines isn't promised. |
| `equal`, `not_equal`, `greater_equal`, `less`, `less_equal`, `logical_and`, `logical_or`, `logical_xor`, `logical_not`, `bitwise_and`, `bitwise_or`, `bitwise_xor`, `bitwise_invert`, `positive`, `ceil`, `floor`, `trunc`, `round`, `isnan`, `isinf`, `isfinite`, `signbit` | Direct | #32 variants. Exact; need only operand domains. `round` ties to even, distinct from `Cast`'s `Rounding`. `isnan` is not "is missing" (#39). |
| `clip` | Compose | `maximum` then `minimum`; scalar promotion as clarified in 2025.12 (PR #47). |
| `conj`, `real`, `imag` | Deferred | Complex dtypes (#26, PR #47). |
| **Indexing** | | |
| `take` | Direct | `Select` with a bound position array on one dimension, #36. |
| `take_along_axis` | New | Not in #36's current scope.¹ |
| **Linear algebra** | | |
| `matrix_transpose` | Direct | Transpose of the last two dimensions, #4. |
| `vecdot` | Compose | `multiply` then `Reduce(sum)`. PR #47 leaves the summation order unspecified (its open question 4), the same as the spec. |
| `matmul`, `tensordot` | Deferred | Semantically `expand_dims` + `multiply` + `sum`, but with an O(n·m·k) intermediate. A contraction primitive waits for a use case such as regridding weights; `Warp` covers regridding today. |
| **Manipulation** | | |
| `broadcast_shapes` | Meta | #29, PR #46. |
| `broadcast_to`, `broadcast_arrays` | Blocked | #46's question 7 decides the representation. See [Questions for review](#questions-for-review). |
| `concat` | New | Variadic (`AtLeast(1)` in PR #45), extent sum along a dimension, promotion across inputs. No design. With symbolic extents it also needs a sum size term, which PR #46's `SizeTerm` doesn't define. |
| `stack` | Compose after New | `expand_dims` on each input, then `concat`. |
| `expand_dims` | New | Adds a length-1 dimension with a new `DimId`. Also `newaxis`. PR #46 names it as an operation #34 needs. |
| `squeeze` | Compose | `Select` with scalar index 0, #3. Only when the extent is proven to be 1; a symbolic extent is rejected. |
| `flip` | Compose | `Select` slice with step −1, #3. |
| `permute_dims`, `moveaxis` | Direct | Transpose, #4. Negative axes (2025.12) resolve at lowering. |
| `reshape` | Blocked | `Reshape`, #5, but the spec reshapes in row-major order and #5 checks only element counts. Blocked on a traversal-order contract. |
| `repeat` | Compose | `Select` with a literal position list (#3 allows repeats).² |
| `roll` | Compose | Literal position list, #3. |
| `tile` | Compose | Literal position lists, #3.³ |
| `unstack` | Compose | One scalar `Select` per output; one root each (PR #45). |
| **Searching** | | |
| `argmax`, `argmin`, `count_nonzero` | Direct | `Reduce` variants (roadmap step 5). Index dtype from PR #47. |
| `nonzero` | Deferred | Its extent is Runtime (`CountNonZero`, PR #46) and its order row-major. Boolean indexing (#36) covers the use case. |
| `searchsorted` | Deferred | Known output shape, but needs a sortedness precondition planning can't check. Label resolution, its main use, is outside the core. |
| `where` | New | Ternary, `Exactly(3)`. See [Questions for review](#questions-for-review). |
| **Sets** | | |
| `unique_all`, `unique_counts`, `unique_inverse`, `unique_values` | Deferred | Extents are Runtime; no data-access use yet. |
| `isin` | Deferred | Known shape, but no data-access use yet. |
| **Sorting** | | |
| `sort`, `argsort` | Deferred | Shape-preserving, but NaN ordering and stability need a contract. |
| **Statistical** | | |
| `sum`, `prod`, `mean`, `min`, `max` | Direct | `Reduce`, roadmap step 5; accumulation proposed in PR #47, to be implemented in #31. |
| `std`, `var` | Direct | `Reduce` with `correction` (xrexpr's `ddof`) as a result-affecting option. |
| `cumulative_sum`, `cumulative_prod` | Blocked | `Scan` ([design.md](design.md#what-to-extract)), which has no issue or design yet. `include_initial` adds one to the extent; on a symbolic extent that needs an offset size term PR #46 doesn't define. |
| **Utility** | | |
| `all`, `any` | Direct | Boolean `Reduce` variants. |
| `diff` | Compose | `subtract` of two shifted slices (#3, #32).⁴ |
| **Constants** | | |
| `e`, `pi`, `inf`, `nan` | Meta | Typed scalars (#26 `Float64Scalar` bits). |
| `newaxis` | New | Same as `expand_dims`. |
| **Inspection** | | |
| `__array_namespace_info__`, `capabilities`, `default_device`, `default_dtypes`, `devices`, `dtypes` | Meta | Host side. `default_dtypes` affects results, so lowering states dtypes explicitly (PR #47). `capabilities` (boolean indexing, data-dependent shapes, max rank) maps to adapter acceptance. |
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

## Counts

The 135 main-namespace functions. The counts leave out the 6 inspection functions,
the constants, operators, array attributes and the two extensions.

| Class | Count |
| --- | --- |
| Direct | 81 |
| Compose | 9 |
| Compose after New | 2 |
| New | 12 |
| Blocked | 5 |
| Meta | 6 |
| Runtime | 0 |
| Deferred | 20 |
| **Total** | **135** |

Runtime applies to rules and array methods, not to any counted function: the
data-dependent functions (`nonzero`, `unique_*`) are Deferred. 63 Direct rows are
`Elementwise` variants. #32 specifies the first three; the rest each need the rules
named in their group.

## Missing concepts, by data-access use case

Only concepts that no design or issue yet covers. Ranked by need in selection,
reduction, alignment and regridding, not by catalogue size.

1. **`concat`** (alignment). Assumes datasets split across files along time reach
   ArrIR as several `Open`s. If the byterefs layout ([chunked-layout.md](chunked-layout.md))
   presents them as one `Open`, this drops below `where`. Needs a sum size term in
   PR #46 for symbolic extents.
2. **`where`** (selection, alignment). Masking by land/sea or quality flags, and
   assigning through a mask.
3. **`expand_dims`** (alignment). PR #46 says named broadcasting (#34) needs it to insert
   length-1 axes. `stack`, `meshgrid` and `tile` also rest on it.
4. **A `Reduce` specification issue** (reduction). Roadmap step 5 exists, and PR #47
   covers accumulation dtypes. `argmax`/`argmin`, `correction`, boolean `all`/`any` and
   `count_nonzero` have no ticket.
5. **A traversal-order contract** (selection). `reshape`, `nonzero` and boolean indexing
   all yield row-major order. #5 needs it before claiming equivalence, and #36 needs it
   for masks over more than one dimension.
6. **`Scan`** (reduction). In the extraction table, with no issue. `include_initial`
   needs an offset size term in PR #46.

Deferred until a use case arrives: shaped constants, contractions (`matmul`,
`tensordot`), sort and set operations, `searchsorted`, the `linalg` and `fft`
extensions, and complex dtypes.

## Questions for review

1. **`broadcast_to`** (owned by #46's question 7). The options are a composition
   (multiply by a `ones` constant), a metadata-only `Broadcast` node, or `expand_dims`
   plus a shape-only broadcast. Recommendation: no composition. Multiplying by ones is
   exact for Bool (as `logical_and`), integers, −0.0 and ±inf, but it fails on four
   counts. Its missingness depends on #39. A `ones` can't be typed when the dtype is
   Unknown. A rewrite folding `x * 1` to `x` would drop the broadcast. It also turns a
   metadata change into arithmetic. Prefer `expand_dims` plus a shape-only broadcast,
   added when a use case needs it.
2. **`where`** (#32). A ternary `Elementwise` variant or its own node? Recommendation: a
   variant. It shares broadcasting and promotion inference with the rest of the family;
   its condition must be Bool.
3. **`concat` first?** Recommendation: yes, if multi-file time series reach ArrIR as
   several `Open`s. Confirm against how the byterefs layout presents them.
4. **Complex dtypes.** The spec requires them; #26 and PR #47 defer them and reject at
   lowering. Recommendation: keep them deferred until a data-access use case (spectra,
   for example). Rejecting them explicitly is honest about the gap.

## Notes for other documents

- [design.md](design.md#conventions) says no operation in the current vocabulary
  reorders dimensions. Transpose (#4) would; #4 should update that line when it lands.
