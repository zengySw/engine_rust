mod args;
mod camera;
mod culling;
mod engine;
mod file_association;
mod inventory;
mod item_registry;
mod menu;
mod mod_converter;
mod modding;
mod paths;
mod player;
mod raytracing;
mod renderer;
mod save;
mod sound;
mod start_menu;
mod world;

fn main() {
    env_logger::init();
    let args = args::Args::parse();
    if let Some(path) = args.import_jar.as_ref() {
        match mod_converter::import_jar_mod(path) {
            Ok(report) => {
                println!(
                    "Imported mod assets:\n  source: {}\n  pack: {}\n  pack entries: {}\n  extracted sounds: {}",
                    report.source.to_string_lossy(),
                    report.output_pack.to_string_lossy(),
                    report.pack_entries,
                    report.extracted_sound_files
                );
            }
            Err(err) => {
                eprintln!("Failed to import mod jar: {err}");
                std::process::exit(1);
            }
        }
        return;
    }
    file_association::ensure_rc_file_association();
    pollster::block_on(run(args));
}

async fn run(args: args::Args) {
    let (engine, event_loop) = engine::Engine::new(args).await;
    engine.run(event_loop);
}
