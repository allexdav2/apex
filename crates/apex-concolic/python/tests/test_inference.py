"""Tests for type inference and proxy argument construction."""
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from apex_symbolic import ConstraintCollector, SymbolicInt
from apex_symbolic.inference import infer_proxy_args


def test_infer_typed_function():
    """int annotations produce SymbolicInt proxies."""

    def func(a: int, b: int) -> int:
        return a + b

    c = ConstraintCollector()
    args = infer_proxy_args(func, c)
    assert len(args) == 2
    assert all(isinstance(v, SymbolicInt) for v in args.values())
    assert args["a"].expr == "a"
    assert args["b"].expr == "b"


def test_infer_untyped_defaults_to_int():
    """Parameters without annotations default to SymbolicInt."""

    def func(x, y):
        return x - y

    c = ConstraintCollector()
    args = infer_proxy_args(func, c)
    assert len(args) == 2
    assert all(isinstance(v, SymbolicInt) for v in args.values())
    assert args["x"].expr == "x"


def test_infer_skips_self():
    """self and cls parameters are skipped."""

    class Foo:
        def method(self, x: int):
            return x

        @classmethod
        def clsmethod(cls, y: int):
            return y

    c = ConstraintCollector()
    args = infer_proxy_args(Foo.method, c)
    assert "self" not in args
    assert len(args) == 1
    assert args["x"].expr == "x"

    args2 = infer_proxy_args(Foo.clsmethod.__func__, c)
    assert "cls" not in args2
    assert len(args2) == 1
    assert args2["y"].expr == "y"
