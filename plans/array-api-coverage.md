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
Every "Direct" row below means *a designed or proposed operation would represent it*.

## Classes

| Class | Meaning |
| --- | --- |
| **Direct** | A designed or proposed operation represents it. |
| **Compose** | A composition of designed or proposed operations represents it, with no loss of meaning. |
| **New** | Needs an operation that no design or issue proposes yet. |
| **Meta** | Answered by lowering from declared metadata; no node. |
| **Runtime** | The result or its shape depends on data. Needs symbolic extents or a host barrier. |
| **Deferred** | Out of scope until a data-access use case needs it. |

Array API axes are positions. ArrIR dimensions are named and ordered. Lowering maps an
axis (negative ones included) to a `DimId` from the input schema: that mapping is
**Meta** for every function with an `axis` argument.

## Semantic rules

| Rule | Class | Representation and evidence |
| --- | --- | --- |
| Broadcasting | Meta | Positional shape inference, #29, proposed in #46 with no `Broadcast` node. Aligned axes must share a `DimId` (#46's `AxisNameMismatch`), so lowering maps positions to names first. Named broadcasting is separate (#34). |
| Type promotion, scalar arguments | Meta | #30, which lists the mixed-kind combinations the spec leaves unspecified as unsupported, with no backend-default fallback. Implemented by #31. |
| Data types | Meta | Bool, int8–64, uint8–64, float32/64 are in #26's `DType`. complex64/128 are **required** by the spec but deferred in #26. |
| Basic indexing (int, slice, ellipsis) | Direct | `Select` positional indexers, #3; conventions in [design.md](design.md#conventions). |
| Integer-array indexing | Direct | The spec's subset of vectorised indexing; #36 compares it with orthogonal indexing and chooses the modes. Slices mixed with integer arrays are unspecified by the spec; reject. |
| Boolean-array indexing | Runtime | #36; the extent is a symbol for the mask's true count (`CountNonZero` in #36, `SymbolOrigin::CountTrue` in #46). A literal mask has a known count (#3). |
| Data-dependent output shapes | Runtime | Symbolic extents (#29/#46, #36). The spec lets libraries omit these; adapters advertise this as a capability. |
| Copies, views, mutation | Deferred | Plans are values, and the proposed node arena is append-only (#45). `copy=` and in-place operators are adapter concerns. |
| Device support | Deferred | Outside the logical core, as #41 requires; no result semantics depend on it. |
| Lazy versus eager | Meta | ArrIR is lazy by construction. Calls that force a value (`__bool__` and so on) are host barriers. |
| Accuracy | Meta | The spec doesn't fix transcendental accuracy. ArrIR keeps evaluation order; fixtures state tolerances (#42). |

## Functions

Each function appears once. Proposed representation is in the third column.

| Functions | Class | Representation and evidence |
| --- | --- | --- |
| **Creation** | | |
| `asarray` | Meta | A binding on `Open`, or a scalar literal (#26 `Scalar`). Embedded arrays are not allowed ([design.md](design.md#decisions)). |
| `from_dlpack` | Deferred | Host interchange, outside the core. |
| `zeros`, `ones`, `full`, `zeros_like`, `ones_like`, `full_like` | New | A constant leaf with declared schema and value; the `_like` forms take shape from an input. No design yet. |
| `empty`, `empty_like` | Deferred | Uninitialised values have no defined meaning. |
| `arange`, `linspace` | New | A generated index sequence. Deferred until coordinate generation needs it; regular grids are declared ([grids.md](grids.md)). |
| `meshgrid` | Compose | `expand_dims` on each input, then `broadcast_to` as below; depends on the New `expand_dims` and constant leaf. |
| `eye`, `tril`, `triu` | Deferred | Compose from `arange`, comparison and `where` once those exist; no data-access use. |
| **Data types** | | |
| `astype` | Direct | `Cast`, [cast.md](cast.md), #38. Stricter than the spec: rounding and out-of-range policies are explicit where the spec leaves them implementation-defined. |
| `can_cast`, `result_type`, `isdtype`, `finfo`, `iinfo`, dtype `__eq__` | Meta | Promotion tables, #30. |
| **Elementwise** | | |
| `add`, `multiply`, `greater` | Direct | First `Elementwise` variants, #32; implemented by #33. |
| `subtract`, `divide`, `floor_divide`, `remainder`, `pow`, `negative`, `positive`, `abs`, `sign`, `square`, `sqrt`, `reciprocal`, `maximum`, `minimum`, `hypot`, `copysign`, `nextafter`, `logaddexp` | Direct | `Elementwise` variants, #32. Each needs its own overflow and non-finite rules before it's accepted. |
| `equal`, `not_equal`, `greater_equal`, `less`, `less_equal` | Direct | Comparison variants, #32. |
| `logical_and`, `logical_or`, `logical_xor`, `logical_not` | Direct | Logical variants, #32. |
| `bitwise_and`, `bitwise_or`, `bitwise_xor`, `bitwise_invert`, `bitwise_left_shift`, `bitwise_right_shift` | Direct | #32 variants; shift-count rules need stating. |
| `exp`, `expm1`, `log`, `log1p`, `log2`, `log10`, `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `sinh`, `cosh`, `tanh`, `asinh`, `acosh`, `atanh` | Direct | #32 mathematical variants; fixtures need tolerances, not bitwise equality. |
| `ceil`, `floor`, `trunc`, `round` | Direct | #32 variants. `round` ties to even; that rounding is distinct from `Cast`'s `Rounding`. |
| `isnan`, `isinf`, `isfinite`, `signbit` | Direct | #32 variants. `isnan` is not "is missing" ([warp.md](warp.md#missing-values), #39). |
| `clip` | Compose | `maximum` then `minimum`; scalar promotion as clarified in 2025.12, #30. |
| `conj`, `real`, `imag` | Deferred | Complex dtypes, #26. |
| **Indexing** | | |
| `take` | Direct | `Select` with a bound position array on one dimension, #36. |
| `take_along_axis` | Direct | #36 vectorised mode, if #36 includes it; otherwise New. |
| **Linear algebra** | | |
| `matrix_transpose` | Compose | Transpose of the last two dimensions, #4. |
| `vecdot` | Compose | `multiply` then `Reduce(sum)` — only once Reduce fixes accumulation order (roadmap step 5, #30). |
| `matmul`, `tensordot` | Deferred | Semantically `expand_dims` + `multiply` + `sum`, but that fixes a summation order and an O(n·m·k) intermediate. A contraction primitive waits for a use case such as regridding weights; `Warp` covers regridding today. |
| **Manipulation** | | |
| `broadcast_shapes` | Meta | #29, #46. |
| `broadcast_to`, `broadcast_arrays` | Compose | #46 (for #29) declines a `Broadcast` node. Multiply by a `ones` constant of the same dtype (`logical_and` with true for Bool): exact apart from NaN payloads, unlike adding zero, which turns −0.0 into +0.0. Needs the New constant leaf. |
| `concat` | New | Variadic (`AtLeast(1)` in #45), extent sum along a dimension, promotion across inputs. No design. |
| `stack` | Compose | `expand_dims` on each input, then `concat`. |
| `expand_dims` | New | Adds a length-1 dimension with a new `DimId`. Also `newaxis`. No design. |
| `squeeze` | Compose | `Select` with scalar index 0 on each length-1 dimension, #3. |
| `flip` | Compose | `Select` slice with step −1, #3. |
| `permute_dims`, `moveaxis` | Direct | Transpose, #4. Negative axes (2025.12) resolve at lowering. |
| `reshape` | Direct | `Reshape`, #5. **Not equivalent yet:** the spec reshapes in row-major order; #5 checks element counts only. A traversal-order contract must exist before any equivalence claim. |
| `repeat` | Compose | `Select` with a literal position list (#3 allows repeats). Array-valued `repeats` is Runtime. |
| `roll`, `tile` | Compose | Literal position lists, #3; `tile` with extra leading dims also needs `expand_dims`. |
| `unstack` | Compose | One scalar `Select` per output; one root each (#45). |
| **Searching** | | |
| `argmax`, `argmin`, `count_nonzero` | Direct | `Reduce` variants (roadmap step 5). Index dtype from #30. |
| `nonzero` | Runtime | Output extent is `CountNonZero`, #36; row-major order, as for `reshape`. Standalone `nonzero` is deferred; boolean indexing is the use case. |
| `searchsorted` | Deferred | Known output shape, but needs a sortedness precondition planning can't check. Label resolution, its main use, is outside the core. |
| `where` | New | Ternary, `Exactly(3)`. Either a ternary #32 variant or its own node: #32 should decide. |
| **Sets** | | |
| `unique_all`, `unique_counts`, `unique_inverse`, `unique_values` | Runtime | Data-dependent extents. Deferred. |
| `isin` | Deferred | Known shape, but no data-access use yet. |
| **Sorting** | | |
| `sort`, `argsort` | Deferred | Shape-preserving, but NaN ordering and stability need a contract. |
| **Statistical** | | |
| `sum`, `prod`, `mean`, `min`, `max` | Direct | `Reduce`, roadmap step 5; dtype and accumulation in #30, #31. |
| `std`, `var` | Direct | `Reduce` with `correction` (xrexpr's `ddof`) as a result-affecting option. |
| `cumulative_sum`, `cumulative_prod` | Direct | `Scan` ([design.md](design.md#what-to-extract)); `include_initial` adds one to the extent. No issue yet. |
| **Utility** | | |
| `all`, `any` | Direct | Boolean `Reduce` variants. |
| `diff` | Compose | `subtract` of two shifted slices (#3, #32); `prepend`/`append` need `concat`. |
| **Constants** | | |
| `e`, `pi`, `inf`, `nan` | Meta | Scalar literals, #26 (`Float64Scalar` bits). |
| `newaxis` | New | Same as `expand_dims`. |
| **Inspection** | | |
| `__array_namespace_info__`, `capabilities`, `default_device`, `default_dtypes`, `devices`, `dtypes` | Meta | Host side. `default_dtypes` affects results, so lowering states dtypes explicitly (#30). `capabilities` (boolean indexing, data-dependent shapes, max rank) maps to adapter acceptance. |
| **Array object** | | |
| `dtype`, `shape`, `ndim`, `size`, `device` | Meta | Schema facts (#26). `shape` may contain unknown extents. |
| `T`, `mT` | Compose | Transpose, #4. |
| Operators (`__add__`, `__lt__`, `__matmul__`, …) | — | Same as their functions above. |
| `__getitem__` | Direct | Indexing rules above. |
| `__setitem__`, in-place operators | Deferred | Mutation. The functional equivalent is `where`. |
| `__bool__`, `__int__`, `__float__`, `__complex__`, `__index__` | Runtime | Force a value: host barrier. |
| `__array_namespace__`, `__dlpack__`, `__dlpack_device__`, `to_device` | Deferred | Host interchange and device. |
| **Extensions** | | |
| `linalg.*` (25 functions: `cholesky`, `cross`, `det`, `diagonal`, `eig`, `eigh`, `eigvals`, `eigvalsh`, `inv`, `matmul`, `matrix_norm`, `matrix_power`, `matrix_rank`, `matrix_transpose`, `outer`, `pinv`, `qr`, `slogdet`, `solve`, `svd`, `svdvals`, `tensordot`, `trace`, `vecdot`, `vector_norm`) | Deferred | Optional extension. The aliases follow their main-namespace rows. |
| `fft.*` (`fft`, `ifft`, `fftn`, `ifftn`, `rfft`, `irfft`, `rfftn`, `irfftn`, `hfft`, `ihfft`, `fftfreq`, `rfftfreq`, `fftshift`, `ifftshift`) | Deferred | Optional extension. `fftshift` would compose as `roll`. |

## Counts

Main namespace functions only (not operators, attributes or extensions):

| Class | Count |
| --- | --- |
| Direct | 83 |
| Compose | 14 |
| New | 11 |
| Meta | 7 |
| Runtime | 5 |
| Deferred | 15 |
| **Total** | **135** |

"Direct" counts proposals; none are implemented in the replacement IR. 63 of the 83
are `Elementwise` variants. #32 specifies the first three; the other 60 each need their
own rules before they're accepted.

## Missing concepts, by data-access use case

Only concepts that no design or issue yet covers. Ranked by need in selection,
reduction, alignment and regridding, not by catalogue size.

1. **`concat`** (alignment). Joining datasets split across files along time is the most
   common multi-source read. #45 already allows `AtLeast(1)` arity. Needs extent and
   promotion rules, and a decision on what happens to coordinates.
2. **`where`** (selection, alignment). Masking (land/sea, quality flags) and the missing
   values an outer join inserts (#34) both need it. #32 should decide whether it's a
   ternary variant.
3. **A `Reduce` specification issue** (reduction). Roadmap step 5 exists, but #30/#31
   cover only dtypes. `argmax`/`argmin`, `correction`, boolean `all`/`any` and
   `count_nonzero` have no ticket.
4. **`expand_dims` and constant leaves** (alignment). Needed for `stack`, `meshgrid`,
   scalar broadcasting and filling absent positions. No design.
5. **A traversal-order contract** (selection). `reshape`, `nonzero` and boolean indexing
   all yield row-major order. #5 must state it before claiming equivalence, and #36
   needs it for masks over more than one dimension.
6. **`Scan`** (reduction). In the extraction table, with no issue.

Deferred until a use case arrives: contractions (`matmul`, `tensordot`), sort and set
operations, `searchsorted`, the `linalg` and `fft` extensions, and complex dtypes. Of
these, complex dtypes are the first likely to be needed, since the spec requires them.

## Notes for other documents

- [design.md](design.md#conventions) says no operation in the current vocabulary
  reorders dimensions. Transpose (#4) would; #4 should update that line when it lands.
