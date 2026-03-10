const { greet, clamp } = require("../index");

test("greet with name", () => {
  expect(greet("Alice", false)).toBe("Hey Alice!");
});

test("clamp within range", () => {
  expect(clamp(5, 0, 10)).toBe(5);
});
