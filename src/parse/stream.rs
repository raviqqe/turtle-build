use combine::{
    easy,
    stream::position::{self, SourcePosition},
};

pub type Stream<'a> = easy::Stream<position::Stream<&'a str, SourcePosition>>;

pub fn stream(source: &str) -> Stream {
    easy::Stream(position::Stream::new(source))
}
