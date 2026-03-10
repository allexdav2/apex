"""Type inference for constructing symbolic proxy arguments.

Uses inspect.signature() and type annotations to create appropriate
symbolic proxies for function parameters.
"""
from __future__ import annotations

import inspect
from collections import OrderedDict
from typing import Callable, Dict

from .proxy import ConstraintCollector, SymbolicInt

_SKIP_PARAMS = {"self", "cls"}


def infer_proxy_args(
    func: Callable, collector: ConstraintCollector
) -> Dict[str, SymbolicInt]:
    """Create symbolic proxy arguments for *func* based on its signature.

    Parameters with ``int`` annotation (or no annotation) default to
    :class:`SymbolicInt`.  ``self`` and ``cls`` parameters are skipped.
    """
    sig = inspect.signature(func)
    result: Dict[str, SymbolicInt] = OrderedDict()

    for name, param in sig.parameters.items():
        if name in _SKIP_PARAMS:
            continue
        # Default everything to SymbolicInt for now; extend later for
        # str/float/bool when those proxy types are implemented.
        result[name] = SymbolicInt(name, collector)

    return result
