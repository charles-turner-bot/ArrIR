"""Elementwise evidence for plans/elementwise.md.

Pins integer wrapping, integer division and remainder (including by zero, which is
ZeroDivisor::Zero), bool arithmetic, NaN comparisons, float floor division, signed
zeros in maximum/minimum, the reassociation counterexample, and xarray's result
names and auxiliary-coordinate handling. Signed-zero results are recorded for arm64
only. Verified with NumPy 2.5.4, xarray 2026.9.0 and dask 2026.8.0 on macOS arm64,
1 November 2026.
"""

import os
import platform
import sys
import warnings

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import dask
import dask.array as da_
import numpy as np
import xarray as xr

from _check import check, finish, header, raises

header(np, xr, dask)
warnings.simplefilter("ignore", RuntimeWarning)

i8 = np.int8

# Integers wrap.
check("int8 127 + 1", int(np.array([127], i8)[0] + i8(1)), -128)
check("abs(int8 -128)", int(np.abs(np.array([-128], i8))[0]), -128)
check("int8 -128 // -1", int((np.array([-128], i8) // np.array([-1], i8))[0]), -128)
check("int8 -128 % -1", int((np.array([-128], i8) % np.array([-1], i8))[0]), 0)
check("int8 2 ** 7", int((np.array([2], i8) ** i8(7))[0]), -128)

# Reassociation is exact only within one dtype: wrapping happens at each node's width.
a, b, c = np.array([127], i8), np.array([1], i8), np.array([0], np.int16)
check("(i8 127 + i8 1) + i16 0", int(((a + b) + c)[0]), -128)
check("i8 127 + (i8 1 + i16 0)", int((a + (b + c))[0]), 128)

# Integer floor division and remainder.
check("-7 // 2", int(np.int32(-7) // np.int32(2)), -4)
check("-7 % 2", int(np.int32(-7) % np.int32(2)), 1)
num = np.array([7, -7, 0], np.int32)
zero = np.zeros(3, np.int32)
check("NumPy x // 0 is 0", (num // zero).tolist(), [0, 0, 0])
check("NumPy x % 0 is 0", (num % zero).tolist(), [0, 0, 0])
check("dask x // 0 is 0", (da_.from_array(num) // da_.from_array(zero)).compute().tolist(), [0, 0, 0])
check("dask x % 0 is 0", (da_.from_array(num) % da_.from_array(zero)).compute().tolist(), [0, 0, 0])
with np.errstate(all="raise"):
    raises("x // 0 under errstate(all='raise')", lambda: num // zero, FloatingPointError)
raises("int ** negative int", lambda: np.array([2], np.int32) ** np.array([-1], np.int32), ValueError)

# Float floor division follows Python, not floor(divide(x1, x2)): deferred in ArrIR.
check("1.0 // 0.1 (NumPy)", float(np.float64(1.0) // np.float64(0.1)), 9.0)
check("floor(1.0 / 0.1)", float(np.floor(np.float64(1.0) / np.float64(0.1))), 10.0)
check("inf // 2.0 is NaN", bool(np.isnan(np.float64(np.inf) // np.float64(2.0))), True)

# bool arithmetic: + is OR, * is AND, - raises.
t, f = np.array([True, True]), np.array([True, False])
check("bool + bool is OR", ((t + f).dtype.name, (t + f).tolist()), ("bool", [True, True]))
check("bool * bool is AND", (t * f).tolist(), [True, False])
raises("bool - bool", lambda: t - f, TypeError)
check("logical_and on integers", bool(np.logical_and(i8(2), i8(0))), False)

# NaN comparisons: false, except not-equal.
nan_one = xr.DataArray([np.nan, 1.0])
check("[nan, 1] > 0", (nan_one > 0).values.tolist(), [False, True])
check("[nan, 1] != 0", (nan_one != 0).values.tolist(), [True, True])

# Rounding functions.
check("ceil keeps integer dtype", np.ceil(np.array([3], np.int32)).dtype.name, "int32")
check("round is half to even", np.round(np.array([0.5, 1.5, 2.5])).tolist(), [0.0, 2.0, 2.0])
check("sign(-0.0) is +0", bool(np.signbit(np.sign(np.float64(-0.0)))), False)

# Signed zeros in maximum/minimum: implementation-defined, recorded for arm64.
if platform.machine() in ("arm64", "aarch64"):
    pz, nz = np.zeros(64), -np.zeros(64)
    for size in (1, 8, 64):
        check(f"arm64 maximum(-0, +0) is +0, size {size}", bool(np.signbit(np.maximum(nz[:size], pz[:size])).any()), False)
        check(f"arm64 maximum(+0, -0) is +0, size {size}", bool(np.signbit(np.maximum(pz[:size], nz[:size])).any()), False)
        check(f"arm64 minimum(+0, -0) is -0, size {size}", bool(np.signbit(np.minimum(pz[:size], nz[:size])).all()), True)
        check(f"arm64 minimum(-0, +0) is -0, size {size}", bool(np.signbit(np.minimum(nz[:size], pz[:size])).all()), True)
else:
    print(f"skip  signed zeros: recorded for arm64, this is {platform.machine()}")

# xarray result names: ArrIR's rule differs, so lowering renames.
ds = xr.Dataset({"t": ("x", [1.0, 2.0]), "u": ("x", [3.0, 4.0])}, coords={"x": [0, 1]})
check("t + 1 keeps t", (ds.t + 1).name, "t")
check("t + t keeps t", (ds.t + ds.t).name, "t")
check("t + u drops the name", (ds.t + ds.u).name, None)
check("t + nd keeps t", (ds.t + np.ones(2)).name, "t")
check("where takes the condition's name", xr.where(ds.t > 1, ds.u, ds.u).name, "t")
check("t + unnamed 0-d drops the name", (ds.t + xr.DataArray(2.0)).name, None)

# Auxiliary coordinates: equal ones kept, conflicting ones dropped.
def aux(values):
    return xr.DataArray([1.0, 2.0], dims="x", coords={"lon": ("x", values)})

check("equal aux coordinate kept", "lon" in (aux([5, 6]) + aux([5, 6])).coords, True)
check("conflicting aux coordinate dropped", "lon" in (aux([5, 6]) + aux([5, 7])).coords, False)

finish()
