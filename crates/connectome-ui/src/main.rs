use std::net::SocketAddr;
use std::path::PathBuf;
use vyrm_store::PersistentEngine;

fn main() {
    if let Err(error) = run() {
        eprintln!("connectome: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut root = PathBuf::from(".");
    let mut db = None;
    let mut bind = "127.0.0.1:4387".to_owned();
    let mut allow_remote = false;
    let mut enable_runners = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => {
                root = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or("--root needs a path")?
            }
            "--db" => db = Some(args.next().map(PathBuf::from).ok_or("--db needs a path")?),
            "--bind" => bind = args.next().ok_or("--bind needs an address")?,
            "--allow-remote" => allow_remote = true,
            "--enable-runners" => enable_runners = true,
            "--help" | "-h" => {
                println!(
                    "connectome [--root PATH] [--db PATH] [--bind 127.0.0.1:4387] [--allow-remote] [--enable-runners]\n\nLocal developer workbench for a vyrm instance. Prompt flights are recorded; frontier CLI execution stays disabled unless --enable-runners is explicit."
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument {arg:?}").into()),
        }
    }
    let address: SocketAddr = bind.parse()?;
    if !address.ip().is_loopback() && !allow_remote {
        return Err(
            "non-loopback binding requires --allow-remote; the workbench has no authentication"
                .into(),
        );
    }
    let (binding, db) =
        connectome_ui::resolve_paths(&root, db.as_deref()).map_err(|error| error.to_string())?;
    let store = PersistentEngine::open(&db)?;
    connectome_ui::serve(store, binding, &bind, enable_runners)
}
