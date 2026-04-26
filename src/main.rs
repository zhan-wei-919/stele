use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    stele::run_demo_app()
}
