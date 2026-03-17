//! Language-specific fuzz harness generators.
//!
//! Each sub-module exposes a `*_fuzz_harness` function that returns a
//! ready-to-compile source file for the corresponding fuzzing backend.
//!
//! | Language | Backend     | Module          |
//! |----------|-------------|-----------------|
//! | C#       | SharpFuzz   | [`csharp`]      |
//! | Swift    | libFuzzer   | [`swift`]       |

pub mod csharp;
pub mod swift;
