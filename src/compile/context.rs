#[derive(Debug, Default)]
pub struct CompileContext {
    build_index: usize,
}

impl CompileContext {
    pub fn new() -> Self {
        Self { build_index: 0 }
    }

    pub fn generate_build_id(&mut self) -> String {
        let index = self.build_index;

        self.build_index += 1;

        index.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_build_ids() {
        let mut context = CompileContext::new();

        assert_eq!(context.generate_build_id(), "0".to_string());
        assert_eq!(context.generate_build_id(), "1".to_string());
        assert_eq!(context.generate_build_id(), "2".to_string());
    }
}
