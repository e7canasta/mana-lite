use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config_path = parse_args()?;
    let config = mana_lite::pico::config::load(&config_path)?;

    let mut app = mana_lite::pico::app::App::bootstrap(&config).await?;
    app.run(&config).await;

    Ok(())
}

fn parse_args() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() == 2 && (args[1] == "--version" || args[1] == "-V") {
        println!("mana-pico v{}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    if args.len() == 3 && args[1] == "--config" {
        return Ok(PathBuf::from(&args[2]));
    }

    if args.len() >= 2 && !args[1].starts_with('-') {
        return Ok(PathBuf::from(&args[1]));
    }

    Err("Usage: mana-pico --config <mana-pico.toml>".into())
}
