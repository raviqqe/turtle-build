mod error;
mod parsers;
mod stream;

pub use self::error::ParseError;
use self::{
    parsers::{dynamic_module, module},
    stream::stream,
};
use crate::ast::{DynamicModule, Module};
use combine::Parser;

pub fn parse(source: &str) -> Result<Module, ParseError> {
    Ok(module().parse(stream(source)).map(|(module, _)| module)?)
}

pub fn parse_dynamic(source: &str) -> Result<DynamicModule, ParseError> {
    Ok(dynamic_module()
        .parse(stream(source))
        .map(|(module, _)| module)?)
}
