mod error;
mod parsers;
mod stream;

use self::error::ParseError;
use self::parsers::module;
use self::stream::stream;
use crate::cfg::Module;
use combine::Parser;

pub fn parse(source: &str) -> Result<Module, ParseError> {
    Ok(module().parse(stream(source)).map(|(module, _)| module)?)
}
