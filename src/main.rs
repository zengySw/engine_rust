mod args;
mod camera;
mod culling;
mod engine;
mod file_association;
mod inventory;
mod item_registry;
mod menu;
mod modding;
mod player;
mod raytracing;
mod renderer;
mod save;
mod sound;
mod world;

fn main() {
    env_logger::init();
    file_association::ensure_rc_file_association();
    let args = args::Args::parse();
    pollster::block_on(run(args));
}

async fn run(args: args::Args) {
    let (engine, event_loop) = engine::Engine::new(args).await;
    engine.run(event_loop);
}
