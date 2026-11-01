"""Shared helpers: compare a host result with the value a plan doc records."""

import platform
import sys

FAILURES = []


def header(*modules):
    names = ", ".join(f"{m.__name__} {m.__version__}" for m in modules)
    print(f"{names}; Python {platform.python_version()}; {platform.system()} {platform.machine()}")


def on_macos_arm64():
    """True where the platform-dependent values were recorded: macOS on Apple silicon."""
    return platform.system() == "Darwin" and platform.machine() == "arm64"


def check(label, actual, expected):
    if actual == expected:
        print(f"ok    {label}")
    else:
        FAILURES.append(label)
        print(f"FAIL  {label}: got {actual!r}, expected {expected!r}")


def raises(label, fn, exc_type):
    try:
        result = fn()
    except exc_type as e:
        print(f"ok    {label}: {type(e).__name__}")
        return
    except Exception as e:  # a different exception is a mismatch too
        FAILURES.append(label)
        print(f"FAIL  {label}: raised {type(e).__name__}, expected {exc_type.__name__}")
        return
    FAILURES.append(label)
    print(f"FAIL  {label}: returned {result!r}, expected {exc_type.__name__}")


def finish():
    if FAILURES:
        print(f"\n{len(FAILURES)} mismatch(es): {', '.join(FAILURES)}")
        sys.exit(1)
    print("\nall checks match")
