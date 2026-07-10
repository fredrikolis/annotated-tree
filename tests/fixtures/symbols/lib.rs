// Lib: demo crate exercising the Rust extractor. | I/O: (Config) -> Engine

pub struct Engine {
    name: String,
}

pub enum State {
    Idle,
    Busy,
}

pub trait Runnable {
    fn run(&self) -> u32;
}

pub fn build(config: &str) -> Engine {
    Engine {
        name: config.to_string(),
    }
}

impl Engine {
    pub fn name(&self) -> &str {
        &self.name
    }
}
