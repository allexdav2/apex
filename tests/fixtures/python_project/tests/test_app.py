"""Partial coverage tests — intentionally skips some branches."""
from src.app import classify_number, safe_divide, fizzbuzz


def test_classify_positive():
    assert classify_number(5) == "small"
    assert classify_number(100) == "large"


def test_safe_divide_normal():
    assert safe_divide(10, 2) == 5.0


def test_fizzbuzz_fizz():
    assert fizzbuzz(3) == "fizz"
    assert fizzbuzz(7) == "7"
