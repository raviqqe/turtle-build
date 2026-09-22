/// Guesses a job limit from a processor count in the same way as Ninja.
pub const fn job_limit(processor_count: usize) -> usize {
    match processor_count {
        0 | 1 => 2,
        2 => 3,
        count => count + 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_processor() {
        assert_eq!(job_limit(0), 2);
    }

    #[test]
    fn one_processor() {
        assert_eq!(job_limit(1), 2);
    }

    #[test]
    fn two_processors() {
        assert_eq!(job_limit(2), 3);
    }

    #[test]
    fn many_processors() {
        assert_eq!(job_limit(8), 10);
    }
}
