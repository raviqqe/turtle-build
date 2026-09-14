#![doc = include_str!("../README.md")]

extern crate alloc;

mod ast;
mod build_graph;
mod compile;
mod context;
mod error;
mod file;
mod hash_type;
mod infrastructure;
mod ir;
mod module_dependency;
mod parse;
mod run;
mod tool;

pub use self::{
    ast::{Module, Statement},
    compile::compile,
    context::Context,
    error::BuildError,
    infrastructure::{FjallDatabase, OsCommandRunner, OsConsole, OsFileSystem},
    module_dependency::{ModuleDependencyMap, validate as validate_module_dependencies},
    parse::parse,
    run::{RunOptions, run},
    tool::clean_dead,
};
