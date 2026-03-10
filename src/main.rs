mod args;
mod camera;
mod culling;
mod engine;
mod renderer;
mod world;

fn main() {
    env_logger::init();
    let args = args::Args::parse();
    pollster::block_on(run(args));
}

async fn run(args: args::Args) {
    let (engine, event_loop) = engine::Engine::new(args).await;
    engine.run(event_loop);
}