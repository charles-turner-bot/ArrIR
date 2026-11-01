"""Broadcasting evidence for plans/shapes.md.

Named operands align by name; positional operands align from the right and adopt
names; n-ary operations (xr.where) combine named operands first, in order of first
appearance, then align positional operands against that combined order. The last
check records the one deliberate difference: xarray lets a positional operand
stretch a named length-1 axis, which ArrIR rejects (PositionalStretchesNamed).
Verified with NumPy 2.5.4 and xarray 2026.9.0, 1 November 2026.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import numpy as np
import xarray as xr

from _check import check, finish, header, raises

header(np, xr)


def named(**sizes):
    return xr.DataArray(np.ones(tuple(sizes.values())), dims=tuple(sizes))


da = named(y=2, x=3)

# Named with positional.
check("da + nd(2, 3)", (da + np.ones((2, 3))).sizes, {"y": 2, "x": 3})
check("nd(2, 3) + da", (np.ones((2, 3)) + da).sizes, {"y": 2, "x": 3})
check("da + nd(3,)", (da + np.ones(3)).sizes, {"y": 2, "x": 3})
check("da + nd(1, 3)", (da + np.ones((1, 3))).sizes, {"y": 2, "x": 3})
check("da + nd(2, 1)", (da + np.ones((2, 1))).sizes, {"y": 2, "x": 3})
check("da + nd() (0-d)", (da + np.ones(())).sizes, {"y": 2, "x": 3})
raises("da + nd(4, 2, 3): PositionalOutranksNamed", lambda: da + np.ones((4, 2, 3)), ValueError)
raises("da(x: 3) + nd(3, 3): PositionalOutranksNamed", lambda: named(x=3) + np.ones((3, 3)), ValueError)
raises("da + nd(2,): IncompatibleExtents", lambda: da + np.ones(2), ValueError)

# Named with named: no length-1 stretching, order is first operand then new dims.
raises("named(y: 1) + named(y: 2)", lambda: named(y=1) + named(y=2), xr.AlignmentError)
check("named(x: 3) + named(y: 2, x: 3) order", (named(x=3) + named(y=2, x=3)).dims, ("x", "y"))

# n-ary: xr.where.
cond = named(x=3) > 0
x = named(x=3)
yx = named(y=2, x=3)
check("where(cond, yx, x)", xr.where(cond, yx, x).dims, ("x", "y"))
check("where(cond, x, yx)", xr.where(cond, x, yx).dims, ("x", "y"))
raises("where(cond, nd(2, 3), yx)", lambda: xr.where(cond, np.ones((2, 3)), yx), ValueError)
raises("where(nd_bool(3,), x, yx)", lambda: xr.where(np.ones(3, bool), x, yx), ValueError)
raises("where(cond, x, nd(2, 3))", lambda: xr.where(cond, x, np.ones((2, 3))), ValueError)

# Stage 1 is stable under regrouping: (a + b) + c and a + (b + c) give one order.
a, b, c = named(x=3), named(y=2), named(z=4, x=3)
check("(a + b) + c order", ((a + b) + c).dims, ("x", "y", "z"))
check("a + (b + c) order", (a + (b + c)).dims, ("x", "y", "z"))

# A positional operand p(3,) aligns with the last axis of the combined named order
# (x, y), whatever its position among the operands. Each condition selects p.
xa, ya = named(x=3), named(y=3)
pos = np.arange(3.0)
picks_p_second = xr.where(xa < 0, ya, pos)  # condition all false: takes pos
picks_p_first = xr.where(xa > 0, pos, ya)  # condition all true: takes pos
for label, r in {"where(x, y, p)": picks_p_second, "where(x, p, y)": picks_p_first}.items():
    check(f"{label}: dims", r.dims, ("x", "y"))
    check(f"{label}: p runs along y", r.isel(x=0).values.tolist(), [0.0, 1.0, 2.0])

# Deliberate difference: xarray stretches a named length-1 axis to fit a
# positional operand and leaves its coordinate at length 1.
stretched = xr.DataArray(np.ones((1, 3)), dims=("y", "x"), coords={"y": [10]}) + np.ones((2, 3))
check("xarray stretches named y: 1 -> 2", stretched.sizes["y"], 2)
check("its y coordinate stays length 1", stretched["y"].size, 1)

finish()
