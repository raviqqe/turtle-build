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
mod job_limit;
mod module_dependency;
mod parse;
mod path_pool;
mod run;
mod tool;

pub use self::{
    ast::{Module, Statement},
    compile::compile,
    context::Context,
    error::BuildError,
    infrastructure::{
        Console, DatabaseError, FileSystem, FjallDatabase, OsCommandRunner, OsConsole,
        OsFileSystem, RedbDatabase,
    },
    job_limit::job_limit,
    module_dependency::{ModuleDependencyMap, validate_modules},
    parse::parse,
    path_pool::PathPool,
    run::{RunOptions, run},
    tool::clean_dead,
};
