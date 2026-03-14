//! Concolic execution engine for APEX — combines concrete execution
//! with symbolic constraint collection for systematic path exploration.

pub mod condition_tree;
pub mod js_conditions;
pub mod python;
pub mod taint;

pub use python::PythonConcolicStrategy;
