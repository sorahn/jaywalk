//! Regenerate checked-in settings references; never touches user configuration.
use std::path::Path;
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let check = match args.as_slice() {
        [] => false,
        [flag] if flag == "--check" => true,
        _ => return Err("usage: cargo run --example settings-reference -- [--check]".into()),
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (file, contents) in [
        (
            "config/user.toml",
            jaywalk::preferences::reference::user_config(),
        ),
        (
            "docs/settings-reference.md",
            jaywalk::preferences::reference::markdown(),
        ),
    ] {
        let path = root.join(file);
        if check {
            if std::fs::read_to_string(&path).map_err(|e| e.to_string())? != contents {
                return Err(format!("{file} is stale; run just settings-reference"));
            }
        } else {
            std::fs::write(&path, contents).map_err(|e| e.to_string())?;
            println!("Updated {file}");
        }
    }
    Ok(())
}
