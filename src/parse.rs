mod error;
mod parser;

pub use self::error::ParseError;
use self::parser::{dynamic_module, module};
use crate::ast::{DynamicModule, Module};

pub fn parse(source: &str) -> Result<Module, ParseError> {
    Ok(module(source).map(|(_, module)| module)?)
}

pub fn parse_dynamic(source: &str) -> Result<DynamicModule, ParseError> {
    Ok(dynamic_module(source).map(|(_, module)| module)?)
}
