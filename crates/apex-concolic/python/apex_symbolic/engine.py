"""Symbolic execution engine with constraint accumulation.

Drives concolic exploration by creating symbolic proxy arguments,
executing the target function, and collecting path constraints.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Callable, List, Optional

from .inference import infer_proxy_args
from .proxy import ConstraintCollector


@dataclass
class ExecutionResult:
    """Result of a single symbolic execution path."""

    constraints: List[str] = field(default_factory=list)
    branch_directions: List[bool] = field(default_factory=list)
    return_value: Any = None
    exception: Optional[Exception] = None


class SymbolicExecutor:
    """Explores a function by injecting symbolic proxy arguments."""

    def explore(self, func: Callable) -> ExecutionResult:
        """Execute *func* once with symbolic arguments, returning the
        accumulated path constraints and concrete result."""
        collector = ConstraintCollector()
        proxy_args = infer_proxy_args(func, collector)

        result = ExecutionResult()
        try:
            result.return_value = func(**proxy_args)
        except Exception as exc:
            result.exception = exc

        result.constraints = list(collector.constraints)
        result.branch_directions = list(collector.directions)
        return result
