/**
 * Module with branches for APEX integration testing.
 */

function greet(name, formal) {
  if (!name) {
    return "Hello, stranger!";
  }
  if (formal) {
    return `Good day, ${name}.`;
  }
  return `Hey ${name}!`;
}

function clamp(value, min, max) {
  if (value < min) return min;
  if (value > max) return max;
  return value;
}

module.exports = { greet, clamp };
