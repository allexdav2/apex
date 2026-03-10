#!/usr/bin/env python3
"""
apex_tracer.py — sys.settrace-based branch tracer for Python targets.

Usage:
    python3 apex_tracer.py <target_dir> <output_json> [test_cmd...]

For every if/while condition executed inside <target_dir>:
  - Records file (relative), line, direction (0=true, 1=false),
    condition text, enclosing function, and scalar locals at that point.

Output JSON:
  {
    "branches": [
      {
        "file": "src/foo.py",
        "line": 42,
        "direction": 0,
        "condition": "x > 0",
        "func": "process",
        "module": "src.foo",
        "args": ["self", "x"],
        "locals": {"x": 5, "n": 3}
      }
    ]
  }
"""
import sys, os, ast, json, types

_target_dir = ""
_branches = []

# Cache parsed ASTs to avoid re-parsing on every line event.
_ast_cache: dict = {}

def _rel(filename: str) -> str:
    return os.path.relpath(filename, _target_dir)

def _in_scope(filename: str) -> bool:
    try:
        rel = os.path.relpath(filename, _target_dir)
        return not rel.startswith("..")
    except ValueError:
        return False

def _get_ast(filename: str):
    if filename in _ast_cache:
        return _ast_cache[filename]
    try:
        with open(filename, encoding="utf-8", errors="replace") as f:
            src = f.read()
        tree = ast.parse(src, filename)
        _ast_cache[filename] = tree
        return tree
    except Exception:
        _ast_cache[filename] = None
        return None

def _find_branch_at(tree, lineno: int):
    """Return the test-expression AST node for an if/while at lineno."""
    for node in ast.walk(tree):
        if isinstance(node, (ast.If, ast.While)) and getattr(node, "lineno", -1) == lineno:
            return node.test
    return None

def _scalar_locals(frame) -> dict:
    out = {}
    for k, v in frame.f_locals.items():
        if k.startswith("_"):
            continue
        if isinstance(v, (int, float, str, bool, type(None))):
            out[k] = v
    return out

def _func_args(frame) -> list:
    code = frame.f_code
    return list(code.co_varnames[: code.co_argcount])

def _module_from_file(filename: str) -> str:
    rel = os.path.relpath(filename, _target_dir)
    mod = rel.replace(os.sep, ".").removesuffix(".py")
    return mod.replace("-", "_")

# ---------------------------------------------------------------------------
# Tracer callbacks
# ---------------------------------------------------------------------------

def _global_tracer(frame, event, arg):
    if event != "call":
        return None
    filename = frame.f_code.co_filename
    if not _in_scope(filename):
        return None
    return _local_tracer

def _local_tracer(frame, event, arg):
    if event != "line":
        return _local_tracer

    filename = frame.f_code.co_filename
    if not _in_scope(filename):
        return _local_tracer

    lineno = frame.f_lineno
    tree = _get_ast(filename)
    if tree is None:
        return _local_tracer

    test_node = _find_branch_at(tree, lineno)
    if test_node is None:
        return _local_tracer

    try:
        # Compile and evaluate the condition in the frame's own namespace.
        code = compile(ast.Expression(body=test_node), filename, "eval")
        result = eval(code, frame.f_globals, frame.f_locals)  # noqa: S307
        direction = 0 if result else 1
    except Exception:
        return _local_tracer

    _branches.append({
        "file": _rel(filename),
        "line": lineno,
        "direction": direction,
        "condition": ast.unparse(test_node),
        "func": frame.f_code.co_name,
        "module": _module_from_file(filename),
        "args": _func_args(frame),
        "locals": _scalar_locals(frame),
    })

    return _local_tracer

# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def run_with_trace(target_dir: str, output_path: str, test_cmd: list):
    global _target_dir
    _target_dir = os.path.abspath(target_dir)
    sys.path.insert(0, _target_dir)

    sys.settrace(_global_tracer)
    try:
        old_argv = sys.argv[:]
        sys.argv = test_cmd if test_cmd else ["pytest", "."]
        try:
            if test_cmd and "pytest" in test_cmd[0]:
                import pytest  # noqa: PLC0415
                pytest.main(test_cmd[1:])
            elif test_cmd:
                import runpy  # noqa: PLC0415
                runpy.run_path(test_cmd[0], run_name="__main__")
            else:
                import pytest  # noqa: PLC0415
                pytest.main(["."])
        except SystemExit:
            pass
        finally:
            sys.argv = old_argv
    finally:
        sys.settrace(None)

    with open(output_path, "w", encoding="utf-8") as f:
        json.dump({"branches": _branches}, f, indent=2)


if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: apex_tracer.py <target_dir> <output_json> [test_cmd...]",
              file=sys.stderr)
        sys.exit(1)
    run_with_trace(sys.argv[1], sys.argv[2], sys.argv[3:])
