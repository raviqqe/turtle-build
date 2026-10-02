#![doc = include_str!("../README.md")]

extern crate alloc;

mod ast;
mod build_graph;
mod build_hash;
mod compile;
mod context;
mod error;
mod file;
mod infrastructure;
mod ir;
mod job_limit;
mod parse;
mod parse_modules;
mod path_pool;
mod run;
mod tool;

pub use self::{
    ast::{Module, Statement},
    compile::compile,
    context::Context,
    error::BuildError,
    infrastructure::{
        Console, Database, DatabaseError, FileSystem, FjallDatabase, LogDatabase, OsCommandRunner,
        OsConsole, OsFileSystem, RedbDatabase,
    },
    job_limit::job_limit,
    parse::parse,
    parse_modules::parse_modules,
    path_pool::PathPool,
    run::{RunOptions, run},
    tool::clean_dead,
};
