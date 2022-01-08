mod error;
mod parsers;
mod stream;

use self::{error::ParseError, parsers::module, stream::stream};
use crate::ir::Module;
use combine::Parser;

pub fn parse(source: &str) -> Result<Module, ParseError> {
    Ok(module().parse(stream(source)).map(|(module, _)| module)?)
}
