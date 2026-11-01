# NumPy and xarray behaviour fixtures

Checks that pin down the host behaviour the decided plan docs rely on. Like
[`fixtures/gdal`](../gdal/README.md), this is evidence for the docs, not #42's
conformance format: these scripts assert what NumPy and xarray do, not what an ArrIR
plan computes. They move into #42's fixture layout once it exists.

| Script | Pins evidence for |
| --- | --- |
| `selection.py` | [select.md](../../plans/select.md): slice clamping, negative and out-of-range indexes, orthogonal versus pointwise shapes, NumPy's adjacency rule, coupled indexers, masks, and why selections don't always merge |
| `broadcasting.py` | [shapes.md](../../plans/shapes.md): named and positional broadcasting, n-ary `xr.where`, and the deliberate difference where xarray stretches a named length-1 axis |
| `cast.py` | [cast.md](../../plans/cast.md): truncation, wrapping, ties to even, overflow to infinity, `bool` rules, `casting=` levels, xarray `astype` and coordinates |
| `elementwise.py` | [elementwise.md](../../plans/elementwise.md): wrapping, integer division and remainder by zero, `bool` arithmetic, NaN comparisons, float floor division, signed zeros, reassociation, xarray names and auxiliary coordinates |

Run them from the repository root with the versions they were verified against.
Python and pandas (an xarray dependency) are pinned too:

```bash
uv run --python 3.13 --with numpy==2.5.4 --with pandas==3.1.0 --with xarray==2026.9.0 \
  python fixtures/numpy_xarray/selection.py
uv run --python 3.13 --with numpy==2.5.4 --with pandas==3.1.0 --with xarray==2026.9.0 \
  python fixtures/numpy_xarray/broadcasting.py
uv run --python 3.13 --with numpy==2.5.4 --with pandas==3.1.0 --with xarray==2026.9.0 \
  --with array-api-strict==2.6.1 python fixtures/numpy_xarray/cast.py
uv run --python 3.13 --with numpy==2.5.4 --with pandas==3.1.0 --with xarray==2026.9.0 \
  --with dask==2026.8.0 python fixtures/numpy_xarray/elementwise.py
```

These use uv because NumPy, xarray and dask install as pure-Python wheels; the GDAL
fixtures use pixi because GDAL needs conda-forge's binary build.

Each script prints the library versions and platform, then one line per check, and
exits non-zero listing every mismatch. A mismatch means a doc's evidence is out of
date: update the doc and any lowering or adapter rule that depends on it first, then
the expected value here.

**Platform-dependent checks.** NumPy's out-of-range float-to-integer results (`cast.py`)
and the sign of zero from `maximum` and `minimum` (`elementwise.py`) were recorded on
macOS arm64 and are skipped everywhere else, Linux aarch64 included. elementwise.md requires the signed-zero check to
be recorded on x86_64 before the NumPy adapter emits `Maximum` or `Minimum` on floats.
