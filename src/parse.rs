mod depfile;
mod error;
mod ninja;

pub use self::error::ParseError;
use self::{
    depfile::depfile,
    ninja::{dynamic_module, module},
};
use crate::ast::{DynamicModule, Module};

pub fn parse(source: &str) -> Result<Module, ParseError> {
    Ok(module(source).map(|(_, module)| module)?)
}

pub fn parse_dynamic(source: &str) -> Result<DynamicModule, ParseError> {
    Ok(dynamic_module(source).map(|(_, module)| module)?)
}

pub fn parse_depfile(source: &str) -> Result<Vec<String>, ParseError> {
    Ok(depfile(source).map(|(_, dependencies)| dependencies)?)
}
