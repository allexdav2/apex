"""CrossHair-style symbolic proxy objects for concolic execution.

Symbolic proxies build SMTLIB2 AST expressions through Python dunder methods,
enabling transparent symbolic tracing of Python functions.
"""
from __future__ import annotations

from typing import List, Union

Concrete = Union[int, float]


class ConstraintCollector:
    """Accumulates SMTLIB2 constraints and branch directions during execution."""

    def __init__(self) -> None:
        self.constraints: List[str] = []
        self.directions: List[bool] = []

    def add_constraint(self, smtlib2: str, direction: bool) -> None:
        self.constraints.append(smtlib2)
        self.directions.append(direction)

    def clear(self) -> None:
        self.constraints.clear()
        self.directions.clear()


def _to_expr(other: object) -> str:
    """Convert a value to its SMTLIB2 expression string."""
    if isinstance(other, SymbolicInt):
        return other.expr
    if isinstance(other, (int, float)):
        return str(other)
    return str(other)


class SymbolicBool:
    """Result of a symbolic comparison. Records constraint when coerced to bool."""

    def __init__(self, smtlib2: str, collector: ConstraintCollector) -> None:
        self.smtlib2 = smtlib2
        self._collector = collector

    def __bool__(self) -> bool:
        self._collector.add_constraint(self.smtlib2, True)
        return True

    def __and__(self, other: SymbolicBool) -> SymbolicBool:
        return SymbolicBool(f"(and {self.smtlib2} {other.smtlib2})", self._collector)

    def __or__(self, other: SymbolicBool) -> SymbolicBool:
        return SymbolicBool(f"(or {self.smtlib2} {other.smtlib2})", self._collector)

    def __invert__(self) -> SymbolicBool:
        return SymbolicBool(f"(not {self.smtlib2})", self._collector)


class SymbolicInt:
    """Symbolic integer that builds SMTLIB2 AST through dunder methods."""

    def __init__(self, expr: str, collector: ConstraintCollector) -> None:
        self.expr = expr
        self._collector = collector

    def _binop(self, op: str, other: object) -> SymbolicInt:
        return SymbolicInt(f"({op} {self.expr} {_to_expr(other)})", self._collector)

    def _rbinop(self, op: str, other: object) -> SymbolicInt:
        return SymbolicInt(f"({op} {_to_expr(other)} {self.expr})", self._collector)

    def _cmp(self, op: str, other: object) -> SymbolicBool:
        return SymbolicBool(f"({op} {self.expr} {_to_expr(other)})", self._collector)

    # --- Arithmetic ---
    def __add__(self, other: object) -> SymbolicInt:
        return self._binop("+", other)

    def __radd__(self, other: object) -> SymbolicInt:
        return self._rbinop("+", other)

    def __sub__(self, other: object) -> SymbolicInt:
        return self._binop("-", other)

    def __rsub__(self, other: object) -> SymbolicInt:
        return self._rbinop("-", other)

    def __mul__(self, other: object) -> SymbolicInt:
        return self._binop("*", other)

    def __rmul__(self, other: object) -> SymbolicInt:
        return self._rbinop("*", other)

    def __floordiv__(self, other: object) -> SymbolicInt:
        return self._binop("div", other)

    def __rfloordiv__(self, other: object) -> SymbolicInt:
        return self._rbinop("div", other)

    def __mod__(self, other: object) -> SymbolicInt:
        return self._binop("mod", other)

    def __rmod__(self, other: object) -> SymbolicInt:
        return self._rbinop("mod", other)

    def __neg__(self) -> SymbolicInt:
        return SymbolicInt(f"(- {self.expr})", self._collector)

    # --- Comparison ---
    def __gt__(self, other: object) -> SymbolicBool:
        return self._cmp(">", other)

    def __ge__(self, other: object) -> SymbolicBool:
        return self._cmp(">=", other)

    def __lt__(self, other: object) -> SymbolicBool:
        return self._cmp("<", other)

    def __le__(self, other: object) -> SymbolicBool:
        return self._cmp("<=", other)

    def __eq__(self, other: object) -> SymbolicBool:  # type: ignore[override]
        return self._cmp("=", other)

    def __ne__(self, other: object) -> SymbolicBool:  # type: ignore[override]
        return SymbolicBool(f"(not (= {self.expr} {_to_expr(other)}))", self._collector)

    # --- Concretization ---
    def __int__(self) -> int:
        return 0

    def __repr__(self) -> str:
        return f"SymbolicInt({self.expr!r})"
