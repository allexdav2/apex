//! Template-based test synthesis for APEX using Tera templates.
//!
//! Generates test files for pytest, Jest, JUnit, and cargo-test.

pub mod jest;
pub mod junit;
pub mod python;
pub mod rust;

pub use jest::JestSynthesizer;
pub use junit::JUnitSynthesizer;
pub use python::PytestSynthesizer;
pub use rust::CargoTestSynthesizer;
