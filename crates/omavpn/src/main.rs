mod client;
mod clipboard;
mod tui;

use std::path::PathBuf;

use anyhow::{bail, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use omavpn_core::models::{Profile, Subscription};
use omavpn_core::storage::Storage;
use omavpn_core::{parser, ping, subscription, DEFAULT_SOCKET_PATH};
use serde_json::json;

use crate::client::DaemonClient;

#[derive(Debug, Parser)]
#[command(
    name = "omavpn",
    version,
    about = "Console/TUI VPN client for Omarchy powered by sing-box"
)]
struct Cli {
    /// Path to omavpnd Unix socket
    #[arg(long, global = true, default_value = DEFAULT_SOCKET_PATH)]
    socket: PathBuf,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Launch interactive TUI (default when no subcommand is given)
    Tui,
    /// Connect to the selected profile (or first profile matching query)
    Up {
        /// Optional substring matching profile name or host
        query: Option<String>,
    },
    /// Disconnect active VPN session
    Down,
    /// Toggle VPN connection on/off (great for Hyprland keybinds & Waybar)
    Toggle,
    /// Show daemon & connection status
    Status {
        /// Output Waybar-compatible JSON
        #[arg(long)]
        waybar: bool,
    },
    /// List saved profiles
    List,
    /// Import VPN URI(s) or base64 subscription text
    Import {
        /// URI string (if omitted, reads from Wayland/X11 clipboard)
        uri: Option<String>,
    },
    /// TCP-ping all saved profiles and update latencies
    Ping,
    /// Manage subscriptions
    Sub {
        #[command(subcommand)]
        action: SubAction,
    },
    /// Show recent sing-box logs from omavpnd
    Logs {
        #[arg(short = 'n', long, default_value_t = 100)]
        lines: usize,
    },
}

#[derive(Debug, Subcommand)]
enum SubAction {
    /// Add a new subscription
    Add {
        name: String,
        url: String,
        /// Optional custom User-Agent header for this subscription
        #[arg(long)]
        user_agent: Option<String>,
    },
    /// Update all enabled subscriptions
    Update,
    /// List subscriptions
    List,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let storage = Storage::open_default()?;
    let client = DaemonClient::new(cli.socket);

    match cli.command.unwrap_or(Commands::Tui) {
        Commands::Tui => tui::run(storage, client).await,
        Commands::Up { query } => cmd_up(&storage, &client, query.as_deref()).await,
        Commands::Down => {
            client.stop().await?;
            println!("Disconnected.");
            Ok(())
        }
        Commands::Toggle => {
            let status = client.status().await?;
            if status.running {
                client.stop().await?;
                println!("Disconnected.");
            } else {
                cmd_up(&storage, &client, None).await?;
            }
            Ok(())
        }
        Commands::Status { waybar } => cmd_status(&client, waybar).await,
        Commands::List => cmd_list(&storage),
        Commands::Import { uri } => cmd_import(&storage, uri.as_deref()),
        Commands::Ping => cmd_ping(&storage).await,
        Commands::Sub { action } => cmd_sub(&storage, action).await,
        Commands::Logs { lines } => {
            let out = client.logs(lines).await?;
            for line in out {
                println!("{line}");
            }
            Ok(())
        }
    }
}

fn find_target_profile(profiles: &[Profile], selected_id: Option<&str>, query: Option<&str>) -> Option<Profile> {
    if let Some(q) = query {
        if let Ok(idx) = q.trim().parse::<usize>() {
            if idx >= 1 && idx <= profiles.len() {
                return Some(profiles[idx - 1].clone());
            }
        }

        let ql = q.to_ascii_lowercase();
        if let Some(p) = profiles.iter().find(|p| {
            p.remarks.to_ascii_lowercase().contains(&ql)
                || p.server.to_ascii_lowercase().contains(&ql)
        }) {
            return Some(p.clone());
        }
    }

    if let Some(id) = selected_id {
        if let Some(p) = profiles.iter().find(|p| p.id == id) {
            return Some(p.clone());
        }
    }

    profiles.first().cloned()
}

async fn cmd_up(storage: &Storage, client: &DaemonClient, query: Option<&str>) -> Result<()> {
    let profiles = storage.load_profiles()?;
    let mut settings = storage.load_settings()?;

    let Some(profile) =
        find_target_profile(&profiles, settings.selected_profile_id.as_deref(), query)
    else {
        bail!("no profiles found — run `omavpn import <uri>` or `omavpn sub add ...` first");
    };

    settings.selected_profile_id = Some(profile.id.clone());
    storage.save_settings(&settings)?;

    let status = client.start(&profile, &settings).await?;
    println!(
        "Connected: {} ({}) [pid={:?}]",
        profile.display_name(),
        settings.mode.as_str(),
        status.pid
    );
    Ok(())
}

async fn cmd_status(client: &DaemonClient, waybar: bool) -> Result<()> {
    match client.status().await {
        Ok(st) if waybar => {
            let payload = if st.running {
                let name = st.active_profile_name.as_deref().unwrap_or("VPN");
                let mode = st.mode.map(|m| m.as_str()).unwrap_or("TUN");
                json!({
                    "text": format!("󰖂 {name}"),
                    "tooltip": format!("omavpn: Connected\nServer: {name}\nMode: {mode}"),
                    "class": "connected"
                })
            } else {
                json!({
                    "text": "󰖂",
                    "tooltip": "omavpn: Disconnected (click to toggle)",
                    "class": "disconnected"
                })
            };
            println!("{payload}");
            Ok(())
        }
        Ok(st) => {
            if st.running {
                println!(
                    "Status : CONNECTED\nServer : {}\nMode   : {}\nPID    : {:?}",
                    st.active_profile_name.as_deref().unwrap_or("unknown"),
                    st.mode.map(|m| m.as_str()).unwrap_or("unknown"),
                    st.pid
                );
            } else {
                println!("Status : DISCONNECTED");
                if let Some(err) = st.last_error {
                    println!("Error  : {err}");
                }
            }
            Ok(())
        }
        Err(err) if waybar => {
            let payload = json!({
                "text": "󰖂 !",
                "tooltip": format!("omavpnd offline: {err}"),
                "class": "offline"
            });
            println!("{payload}");
            Ok(())
        }
        Err(err) => Err(err),
    }
}

fn cmd_list(storage: &Storage) -> Result<()> {
    let profiles = storage.load_profiles()?;
    let settings = storage.load_settings()?;
    if profiles.is_empty() {
        println!("No profiles saved.");
        return Ok(());
    }

    for (i, p) in profiles.iter().enumerate() {
        let sel = if settings.selected_profile_id.as_deref() == Some(p.id.as_str()) {
            "*"
        } else {
            " "
        };
        let ping = match p.latency_ms {
            Some(ms) if ms >= 0 => format!("{ms}ms"),
            Some(_) => "timeout".to_string(),
            None => "-".to_string(),
        };
        println!(
            "{sel} {:>2}. [{}] {:<28} {}:{} ({ping})",
            i + 1,
            p.protocol.as_str(),
            p.display_name(),
            p.server,
            p.port
        );
    }
    Ok(())
}

fn cmd_import(storage: &Storage, uri: Option<&str>) -> Result<()> {
    let raw = match uri {
        Some(s) => s.to_string(),
        None => clipboard::read_clipboard()?,
    };
    let imported = parser::parse_batch(&raw);
    if imported.is_empty() {
        bail!("no valid VPN URIs found in input");
    }
    let count = imported.len();
    let mut profiles = storage.load_profiles()?;
    profiles.extend(imported);
    storage.save_profiles(&profiles)?;
    println!("Imported {count} profile(s).");
    Ok(())
}

async fn cmd_ping(storage: &Storage) -> Result<()> {
    let mut profiles = storage.load_profiles()?;
    if profiles.is_empty() {
        println!("No profiles to ping.");
        return Ok(());
    }
    let results = ping::ping_all(&profiles, 16, 2000).await;
    for (id, ms) in results {
        if let Some(p) = profiles.iter_mut().find(|x| x.id == id) {
            p.latency_ms = Some(ms);
        }
    }
    storage.save_profiles(&profiles)?;
    cmd_list(storage)
}

async fn cmd_sub(storage: &Storage, action: SubAction) -> Result<()> {
    match action {
        SubAction::Add {
            name,
            url,
            user_agent,
        } => {
            let Some(clean_url) = subscription::extract_subscription_url(&url) else {
                bail!("invalid subscription URL (expected http(s):// or happ://add/https://...)");
            };
            let mut subs = storage.load_subscriptions()?;
            let mut sub = Subscription::new(name.clone(), clean_url);
            sub.user_agent = user_agent;
            subs.push(sub);
            storage.save_subscriptions(&subs)?;
            println!("Added subscription '{name}'. Run `omavpn sub update` to fetch servers.");
            Ok(())
        }
        SubAction::List => {
            let subs = storage.load_subscriptions()?;
            if subs.is_empty() {
                println!("No subscriptions saved.");
                return Ok(());
            }
            for sub in subs {
                println!("{} — {}", sub.name, sub.url);
            }
            Ok(())
        }
        SubAction::Update => {
            let mut subs = storage.load_subscriptions()?;
            let mut profiles = storage.load_profiles()?;
            for sub in &mut subs {
                if !sub.enabled {
                    continue;
                }
                match subscription::fetch_subscription(sub).await {
                    Ok(new_profiles) => {
                        profiles.retain(|p| p.subscription_id.as_deref() != Some(sub.id.as_str()));
                        println!("Updated '{}': {} server(s)", sub.name, new_profiles.len());
                        profiles.extend(new_profiles);
                        sub.last_updated = Some(Utc::now());
                    }
                    Err(err) => {
                        eprintln!("Failed '{}': {err}", sub.name);
                    }
                }
            }
            storage.save_subscriptions(&subs)?;
            storage.save_profiles(&profiles)?;
            Ok(())
        }
    }
}
