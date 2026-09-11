mod depfile;
mod error;
mod ninja;

use self::ninja::{dynamic_module, module};
pub use self::{depfile::parse_depfile, error::ParseError};
use crate::ast::{DynamicModule, Module};

pub fn parse(source: &str) -> Result<Module, ParseError> {
    Ok(module(source).map(|(_, module)| module)?)
}

pub fn parse_dynamic(source: &str) -> Result<DynamicModule, ParseError> {
    Ok(dynamic_module(source).map(|(_, module)| module)?)
}
