//! fitd compiles .fitd text to laid-out SVG, PNG, PDF, and TikZ; `compile` is the shared pipeline.

pub mod ast;
pub mod attrs;
pub mod check;
pub mod compile;
pub mod csp;
pub mod diag;
pub mod emit;
pub mod error;
#[cfg(test)]
mod fuzz;
pub mod geom;
pub mod graph;
pub mod import;
pub mod include;
pub mod key;

pub mod lanes;
pub mod layered;
pub mod layout;
pub mod lex;
pub mod lsp;
pub mod macros;
pub mod metrics;
pub mod parse;
pub mod quality;
pub mod radial;
pub mod sema;
pub mod split;
pub mod stdlib;
pub mod theme;
pub mod tree;
pub mod venn;
