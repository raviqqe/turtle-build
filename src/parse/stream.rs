use combine::easy;

pub type Stream<'a> = easy::Stream<&'a str>;

pub fn stream(source: &str) -> Stream {
    easy::Stream(source)
}
