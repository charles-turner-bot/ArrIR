"""Positional selection evidence for plans/select.md.

Pins slice clamping, negative indexes, out-of-range errors, orthogonal versus
pointwise result shapes, and NumPy's adjacency rule (integer scalars count as
advanced indices). Verified with NumPy 2.5.4 and xarray 2026.9.0, 1 November 2026.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import numpy as np
import xarray as xr

from _check import check, finish, header, raises

header(np, xr)

nd = np.arange(12).reshape(3, 4)
da = xr.DataArray(nd, dims=("y", "x"))
nd3 = np.arange(24).reshape(2, 3, 4)
da3 = xr.DataArray(nd3, dims=("a", "b", "c"))


def p(values, dim="p"):
    return xr.DataArray(values, dims=dim)


# Slices clamp; negative positions count from the end; empty slices are valid.
x = np.arange(5)
check("slice clamps: x[1:100]", x[1:100].tolist(), [1, 2, 3, 4])
check("negative start: x[-2:]", x[-2:].tolist(), [3, 4])
check("negative step reverses: x[::-2]", x[::-2].tolist(), [4, 2, 0])
check("empty slice: x[3:1]", x[3:1].shape, (0,))
check("xarray slice clamps", da.isel(x=slice(1, 100)).sizes, {"y": 3, "x": 3})

# Out-of-range scalars and positions raise.
raises("da.isel(x=5)", lambda: da.isel(x=5), IndexError)
raises("da.isel(x=[0, 9])", lambda: da.isel(x=[0, 9]), IndexError)
raises("nd[[0, 9]]", lambda: nd[[0, 9]], IndexError)

# Orthogonal (xarray lists, np.ix_) versus pointwise (NumPy advanced indexing).
r = da.isel(y=[0, 2], x=[1, 3])
check("orthogonal lists: dims", r.dims, ("y", "x"))
check("orthogonal lists: values", r.values.ravel().tolist(), [1, 3, 9, 11])
check("np.ix_ is orthogonal", nd[np.ix_([0, 2], [1, 3])].shape, (2, 2))
r = da.isel(y=p([0, 2]), x=p([1, 3]))
check("pointwise isel: dims", r.dims, ("p",))
check("pointwise isel: values", r.values.tolist(), [1, 11])
check("NumPy advanced indexing is pointwise", nd[[0, 2], [1, 3]].tolist(), [1, 11])

# Where the pointwise axes go.
check("adjacent arrays stay in place (xarray)", da3.isel(b=p([0, 1]), c=p([1, 3])).dims, ("a", "p"))
check("adjacent arrays stay in place (NumPy)", nd3[:, [0, 1], [1, 3]].shape, (2, 2))
check("non-adjacent arrays go first (xarray)", da3.isel(a=p([0, 1]), c=p([1, 3])).dims, ("p", "b"))
check("non-adjacent arrays go first (NumPy)", nd3[[0, 1], :, [1, 3]].shape, (2, 3))
check("integer scalar counts as advanced: nd3[[0,1], :, 0]", nd3[[0, 1], :, 0].shape, (2, 3))
check("integer scalar counts as advanced: nd3[0, :, [0,1]]", nd3[0, :, [0, 1]].shape, (2, 3))

# Indexer dims broadcast by name; distinct 1-D indexers act orthogonally.
check("indexer dims broadcast: (a, p, q)", da3.isel(b=p([0, 1]), c=p([1, 2, 3], "q")).dims, ("a", "p", "q"))
check("distinct 1-D indexers: (p, b, q)", da3.isel(a=p([0, 1]), c=p([1, 2, 3], "q")).dims, ("p", "b", "q"))
check("list keeps the indexed name: (y, p)", da.isel(y=[0, 2], x=p([1, 3])).dims, ("y", "p"))
check("same-name indexers are pointwise: (p, b)", da3.isel(a=p([0, 1]), c=p([1, 2])).dims, ("p", "b"))
raises("mismatched indexer lengths", lambda: da3.isel(b=p([0, 1]), c=p([0, 1, 2])), IndexError)

# An indexer named after another axis gathers along it (CoupledIndexer in ArrIR).
r = da.isel(x=xr.DataArray([3, 0, 1], dims="y"))
check("coupled indexer gathers along y", (r.dims, r.values.tolist()), (("y",), [3, 4, 9]))

# Masks.
check("orthogonal bool mask", da.isel(x=np.array([True, False, True, False])).sizes, {"y": 3, "x": 2})
raises("2-D mask on a DataArray", lambda: da[np.ones((3, 4), bool)], IndexError)
check("2-D mask on NumPy is row-major 1-D", nd[nd % 2 == 0].tolist(), [0, 2, 4, 6, 8, 10])

# Merging selections is not exact for every length (pushdown rule 3).
raises("x[-5:][[3]] at length 3", lambda: np.arange(3)[-5:][[3]], IndexError)
check("x[-5:][[3]] at length 8", np.arange(8)[-5:][[3]].tolist(), [6])

finish()
