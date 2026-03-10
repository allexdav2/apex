"""Tests for the symbolic execution engine."""
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from apex_symbolic.engine import ExecutionResult, SymbolicExecutor


def simple_branch(x: int) -> str:
    if x > 0:
        return "positive"
    return "non-positive"


def test_executor_runs_function():
    executor = SymbolicExecutor()
    result = executor.explore(simple_branch)
    assert isinstance(result, ExecutionResult)
    assert result.exception is None


def test_executor_returns_path_constraints():
    executor = SymbolicExecutor()
    result = executor.explore(simple_branch)
    assert len(result.constraints) >= 1
    # The constraint should be an SMTLIB2 string mentioning "x"
    assert any("x" in c for c in result.constraints)
    assert "(> x 0)" in result.constraints[0]


def test_executor_result_has_concrete_output():
    executor = SymbolicExecutor()
    result = executor.explore(simple_branch)
    # The function should have returned a concrete value
    assert result.return_value is not None
    assert isinstance(result.return_value, str)
