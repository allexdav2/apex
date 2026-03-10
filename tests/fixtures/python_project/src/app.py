"""Sample application with 10 branch directions for APEX testing."""


def classify_number(n):
    """Classify a number into categories."""
    if n < 0:
        return "negative"
    elif n == 0:
        return "zero"
    elif n < 10:
        return "small"
    else:
        return "large"


def safe_divide(a, b):
    """Divide a by b with error handling."""
    if b == 0:
        return None
    result = a / b
    if result < 0:
        return -result
    return result


def fizzbuzz(n):
    """Classic fizzbuzz with branches."""
    if n % 15 == 0:
        return "fizzbuzz"
    elif n % 3 == 0:
        return "fizz"
    elif n % 5 == 0:
        return "buzz"
    else:
        return str(n)
