"""Tests for SymbolicInt, SymbolicBool, and ConstraintCollector."""
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from apex_symbolic import ConstraintCollector, SymbolicBool, SymbolicInt


def _fresh_collector():
    c = ConstraintCollector()
    c.clear()
    return c


def test_symbolic_int_creation():
    c = _fresh_collector()
    x = SymbolicInt("x", c)
    assert x.expr == "x"
    assert x._collector is c


def test_symbolic_int_gt():
    c = _fresh_collector()
    x = SymbolicInt("x", c)
    result = x > 0
    assert isinstance(result, SymbolicBool)
    assert "(> x 0)" in result.smtlib2


def test_symbolic_int_lt():
    c = _fresh_collector()
    x = SymbolicInt("x", c)
    result = x < 10
    assert isinstance(result, SymbolicBool)
    assert "(< x 10)" in result.smtlib2


def test_symbolic_int_eq():
    c = _fresh_collector()
    x = SymbolicInt("x", c)
    result = x == 5
    assert isinstance(result, SymbolicBool)
    assert "(= x 5)" in result.smtlib2


def test_symbolic_int_ne():
    c = _fresh_collector()
    x = SymbolicInt("x", c)
    result = x != 3
    assert isinstance(result, SymbolicBool)
    assert "(not (= x 3))" in result.smtlib2


def test_symbolic_int_add():
    c = _fresh_collector()
    x = SymbolicInt("x", c)
    y = x + 1
    assert isinstance(y, SymbolicInt)
    assert y.expr == "(+ x 1)"


def test_symbolic_int_sub():
    c = _fresh_collector()
    x = SymbolicInt("x", c)
    y = x - 2
    assert isinstance(y, SymbolicInt)
    assert y.expr == "(- x 2)"


def test_symbolic_int_mul():
    c = _fresh_collector()
    x = SymbolicInt("x", c)
    y = x * 3
    assert isinstance(y, SymbolicInt)
    assert y.expr == "(* x 3)"


def test_symbolic_int_bool_queries_solver():
    """When __bool__ is called on a SymbolicBool, it records the constraint
    and returns True (taking the true-branch by default)."""
    c = _fresh_collector()
    x = SymbolicInt("x", c)
    cond = x > 0
    result = bool(cond)
    assert result is True
    assert len(c.constraints) == 1
    assert "(> x 0)" in c.constraints[0]
    assert c.directions == [True]


def test_constraint_collector_clear():
    c = _fresh_collector()
    x = SymbolicInt("x", c)
    bool(x > 0)
    assert len(c.constraints) == 1
    c.clear()
    assert len(c.constraints) == 0
    assert len(c.directions) == 0
