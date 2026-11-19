//! One executable: GUI when launched normally, CLI with subcommands.
mod i18n;
mod ui;
use clap::{Parser, Subcommand, ValueEnum};
use glintvnc_core::{configuration as conf, protocol, secrets};
use std::{
    error::Error,
    io::{self, Read},
    sync::mpsc::sync_channel,
    time::Duration,
};

#[derive(Parser)]
#[command(
    name = "glintvnc",
    version,
    about = "Rust-based VNC viewer for Intel macOS 13+"
)]
struct Args {
    #[command(subcommand)]
    command: Option<Cmd>,
}
#[derive(Subcommand)]
enum Cmd {
    /// Open graphical viewer for a saved profile
    Connect { selector: String },
    /// Manage profiles; passwords NEVER appear in JSON
    Profiles {
        #[command(subcommand)]
        action: ProfileAction,
    },
    /// Read and update settings used by both GUI and CLI
    Settings {
        #[command(subcommand)]
        action: SettingAction,
    },
    /// Store or delete a password in macOS Keychain
    Secret {
        #[command(subcommand)]
        action: SecretAction,
    },
    /// Check connection/auth and server display metadata without opening a viewer
    Probe {
        selector: String,
        #[arg(long, default_value_t = 12)]
        timeout_secs: u64,
    },
}
#[derive(Subcommand)]
enum ProfileAction {
    List {
        #[arg(long)]
        json: bool,
    },
    Add {
        name: String,
        host: String,
        #[arg(long, default_value_t = 5900)]
        port: u16,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        allow_legacy_unencrypted: bool,
    },
    Show {
        selector: String,
    },
    Remove {
        selector: String,
    },
    Set {
        selector: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        fullscreen: Option<bool>,
        #[arg(long)]
        reconnect: Option<bool>,
        #[arg(long, value_enum)]
        keymap: Option<Keymap>,
        #[arg(long, value_enum)]
        scale: Option<Scale>,
        #[arg(long, value_enum)]
        security: Option<Security>,
    },
}
#[derive(Clone, ValueEnum)]
enum Scale {
    Fit,
    PixelPerfect,
    Stretch,
}
#[derive(Clone, ValueEnum)]
enum Security {
    Localhost,
    Legacy,
}
#[derive(Clone, ValueEnum)]
enum Keymap {
    Linux,
    Windows,
    Identity,
}
#[derive(Subcommand)]
enum SettingAction {
    Get { key: String },
    Set { key: String, value: String },
    List,
}
#[derive(Subcommand)]
enum SecretAction {
    Set {
        selector: String,
        #[arg(long)]
        stdin: bool,
    },
    Delete {
        selector: String,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("glintvnc: {e}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let path = conf::data_path()?;
    let mut store = conf::load(&path)?;
    match args.command {
        None => ui::launch(store, path, None)?,
        Some(Cmd::Connect { selector }) => {
            let id = conf::get_profile(&store, &selector)
                .ok_or("profile not found")?
                .id
                .clone();
            ui::launch(store, path, Some(id))?;
        }
        Some(Cmd::Profiles { action }) => match action {
            ProfileAction::List { json } => {
                if json {
                    println!("{}", serde_json::to_string_pretty(&store.profiles)?);
                } else {
                    for p in &store.profiles {
                        println!("{}  {:25}  {}:{}", p.id, p.name, p.host, p.port);
                    }
                }
            }
            ProfileAction::Add {
                name,
                host,
                port,
                username,
                allow_legacy_unencrypted,
            } => {
                let p = conf::Profile {
                    id: conf::make_id(),
                    fullscreen_on_connect: store.settings.default_fullscreen,
                    name,
                    host,
                    port,
                    username,
                    security: if allow_legacy_unencrypted {
                        conf::SecurityPolicy::AllowLegacyUnencrypted
                    } else {
                        conf::SecurityPolicy::LocalhostOnly
                    },
                    ..conf::Profile::default()
                };
                p.validate()?;
                println!("Created profile {}", p.id);
                store.profiles.push(p);
                conf::save(&path, &store)?;
            }
            ProfileAction::Show { selector } => {
                let p = conf::get_profile(&store, &selector).ok_or("profile not found")?;
                println!("{}", serde_json::to_string_pretty(p)?);
            }
            ProfileAction::Remove { selector } => {
                let p = conf::get_profile(&store, &selector).ok_or("profile not found")?;
                let id = p.id.clone();
                store.profiles.retain(|p| p.id != id);
                conf::save(&path, &store)?;
                // Deleting secrets is best-effort: profile deletion is never undone if Keychain is locked.
                if let Err(e) = secrets::delete_password(&id) {
                    eprintln!("Keychain cleanup warning: {e}");
                }
                println!("Removed {id}");
            }
            ProfileAction::Set {
                selector,
                name,
                host,
                port,
                username,
                fullscreen,
                reconnect,
                keymap,
                scale,
                security,
            } => {
                let p = store
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == selector || p.name == selector)
                    .ok_or("profile not found")?;
                if let Some(x) = name {
                    p.name = x;
                }
                if let Some(x) = host {
                    p.host = x;
                }
                if let Some(x) = port {
                    p.port = x;
                }
                if let Some(x) = username {
                    p.username = Some(x);
                }
                if let Some(x) = fullscreen {
                    p.fullscreen_on_connect = x;
                }
                if let Some(x) = reconnect {
                    p.auto_reconnect = x;
                }
                if let Some(x) = keymap {
                    p.key_preset = match x {
                        Keymap::Linux => conf::KeyPreset::MacToLinux,
                        Keymap::Windows => conf::KeyPreset::MacToWindows,
                        Keymap::Identity => conf::KeyPreset::Identity,
                    };
                }
                if let Some(x) = scale {
                    p.scale_mode = match x {
                        Scale::Fit => conf::ScaleMode::Fit,
                        Scale::PixelPerfect => conf::ScaleMode::PixelPerfect,
                        Scale::Stretch => conf::ScaleMode::Stretch,
                    };
                }
                if let Some(x) = security {
                    p.security = match x {
                        Security::Localhost => conf::SecurityPolicy::LocalhostOnly,
                        Security::Legacy => conf::SecurityPolicy::AllowLegacyUnencrypted,
                    };
                }
                p.validate()?;
                conf::save(&path, &store)?;
                println!("Profile updated");
            }
        },
        Some(Cmd::Settings { action }) => match action {
            SettingAction::List => println!("{}", serde_json::to_string_pretty(&store.settings)?),
            SettingAction::Get { key } => println!("{}", setting_get(&store.settings, &key)?),
            SettingAction::Set { key, value } => {
                setting_set(&mut store.settings, &key, &value)?;
                conf::save(&path, &store)?;
                println!("Updated {key}");
            }
        },
        Some(Cmd::Secret { action }) => match action {
            SecretAction::Set { selector, stdin } => {
                let p = conf::get_profile(&store, &selector).ok_or("profile not found")?;
                // For noninteractive automation, pipe via stdin. Never use command-line --password.
                let pass = if stdin {
                    let mut s = String::new();
                    io::stdin().read_to_string(&mut s)?;
                    s.trim_end_matches(['\r', '\n']).to_string()
                } else {
                    rpassword::prompt_password("VNC password: ")?
                };
                secrets::set_password(&p.id, &pass)?;
                println!("Keychain entry saved");
            }
            SecretAction::Delete { selector } => {
                let p = conf::get_profile(&store, &selector).ok_or("profile not found")?;
                secrets::delete_password(&p.id)?;
                println!("Keychain entry deleted");
            }
        },
        Some(Cmd::Probe {
            selector,
            timeout_secs,
        }) => {
            let p = conf::get_profile(&store, &selector)
                .ok_or("profile not found")?
                .clone();
            let password = secrets::get_password(&p.id)?;
            let (evt_tx, evt_rx) = sync_channel(2);
            let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                let result = match rt {
                    Ok(rt) => rt.block_on(protocol::run_once(
                        p,
                        password,
                        cmd_rx,
                        evt_tx.clone(),
                        None,
                    )),
                    Err(e) => Err(protocol::Error::Io(e)),
                };
                if let Err(e) = result {
                    let _ = evt_tx.send(protocol::Event::Closed {
                        message: e.to_string(),
                        retryable: e.retryable(),
                    });
                }
            });
            let until = std::time::Instant::now() + Duration::from_secs(timeout_secs.max(1));
            loop {
                let time = until.saturating_duration_since(std::time::Instant::now());
                if time.is_zero() {
                    return Err("connection probe timed out".into());
                }
                match evt_rx.recv_timeout(time)? {
                    protocol::Event::Connected {
                        name,
                        width,
                        height,
                    } => {
                        println!("Connected: {name} ({width}x{height}); server metadata verified");
                        break;
                    }
                    protocol::Event::Closed { message, .. } => return Err(message.into()),
                    _ => {}
                }
            }
        }
    }
    Ok(())
}
fn setting_get(s: &conf::AppSettings, key: &str) -> Result<String, Box<dyn Error>> {
    Ok(match key {
        "theme" => s.theme.clone(),
        "language" => s.language.clone(),
        "default_fullscreen" => s.default_fullscreen.to_string(),
        "reconnect_max_attempts" => s.reconnect_max_attempts.to_string(),
        "telemetry" => s.telemetry.to_string(),
        "scroll_speed" => s.scroll_speed.to_string(),
        "clipboard_sync" => s.clipboard_sync.to_string(),
        _ => return Err("unknown setting".into()),
    })
}
fn setting_set(s: &mut conf::AppSettings, key: &str, value: &str) -> Result<(), Box<dyn Error>> {
    match key {
        "theme" => {
            if !["dark", "light", "system"].contains(&value) {
                return Err("theme must be dark/light/system".into());
            }
            s.theme = value.into();
        }
        "language" => {
            if !["ja", "en"].contains(&value) {
                return Err("language must be ja/en".into());
            }
            s.language = value.into();
        }
        "default_fullscreen" => s.default_fullscreen = value.parse()?,
        "reconnect_max_attempts" => s.reconnect_max_attempts = value.parse()?,
        "telemetry" => s.telemetry = value.parse()?,
        "scroll_speed" => {
            let v: f32 = value.parse()?;
            if !v.is_finite() || !(0.25..=4.0).contains(&v) {
                return Err("scroll_speed must be a number between 0.25 and 4.0".into());
            }
            s.scroll_speed = v;
        }
        "clipboard_sync" => s.clipboard_sync = value.parse()?,
        _ => return Err("unknown setting".into()),
    }
    Ok(())
}
