use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use omavpn_core::models::{AppSettings, DaemonStatus, Profile, Subscription};
use omavpn_core::{parser, ping, storage::Storage, subscription};

use crate::client::DaemonClient;
use crate::clipboard;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Servers,
    Subscriptions,
    Logs,
    Settings,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Servers, Tab::Subscriptions, Tab::Logs, Tab::Settings];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Servers => "1 Servers",
            Tab::Subscriptions => "2 Subscriptions",
            Tab::Logs => "3 Logs",
            Tab::Settings => "4 Settings",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Tab::Servers => Tab::Subscriptions,
            Tab::Subscriptions => Tab::Logs,
            Tab::Logs => Tab::Settings,
            Tab::Settings => Tab::Servers,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Tab::Servers => Tab::Settings,
            Tab::Subscriptions => Tab::Servers,
            Tab::Logs => Tab::Subscriptions,
            Tab::Settings => Tab::Logs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Search,
    AddLink,
    AddSubName,
    AddSubUrl { name: String },
}

pub struct App {
    pub storage: Storage,
    pub client: DaemonClient,
    pub tab: Tab,
    pub input_mode: InputMode,
    pub input_buffer: String,
    pub search_query: String,

    pub profiles: Vec<Profile>,
    pub subscriptions: Vec<Subscription>,
    pub settings: AppSettings,
    pub daemon_status: DaemonStatus,
    pub daemon_reachable: bool,
    pub logs: Vec<String>,

    pub selected_server: usize,
    pub selected_sub: usize,
    pub selected_setting: usize,
    pub log_scroll: usize,

    pub status_message: String,
    pub should_quit: bool,
}

impl App {
    pub async fn new(storage: Storage, client: DaemonClient) -> anyhow::Result<Self> {
        let profiles = storage.load_profiles()?;
        let subscriptions = storage.load_subscriptions()?;
        let settings = storage.load_settings()?;

        let (daemon_status, daemon_reachable, status_message) = match client.status().await {
            Ok(st) => (st, true, "Ready. Press Enter to connect, ? for help".to_string()),
            Err(err) => (
                DaemonStatus::default(),
                false,
                format!("Daemon offline: {err}"),
            ),
        };

        let selected_server = settings
            .selected_profile_id
            .as_deref()
            .and_then(|id| profiles.iter().position(|p| p.id == id))
            .unwrap_or(0);

        Ok(Self {
            storage,
            client,
            tab: Tab::Servers,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            search_query: String::new(),
            profiles,
            subscriptions,
            settings,
            daemon_status,
            daemon_reachable,
            logs: Vec::new(),
            selected_server,
            selected_sub: 0,
            selected_setting: 0,
            log_scroll: 0,
            status_message,
            should_quit: false,
        })
    }

    pub fn filtered_profiles(&self) -> Vec<(usize, &Profile)> {
        let q = self.search_query.trim().to_ascii_lowercase();
        self.profiles
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                if q.is_empty() {
                    true
                } else {
                    p.remarks.to_ascii_lowercase().contains(&q)
                        || p.server.to_ascii_lowercase().contains(&q)
                        || p.protocol.as_str().to_ascii_lowercase().contains(&q)
                }
            })
            .collect()
    }

    pub async fn refresh_daemon(&mut self) {
        match self.client.status().await {
            Ok(st) => {
                self.daemon_status = st;
                self.daemon_reachable = true;
            }
            Err(_) => {
                self.daemon_status = DaemonStatus::default();
                self.daemon_reachable = false;
            }
        }

        if self.tab == Tab::Logs && self.daemon_reachable {
            if let Ok(lines) = self.client.logs(300).await {
                self.logs = lines;
            }
        }
    }

    pub async fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        match self.input_mode.clone() {
            InputMode::Normal => self.handle_normal_key(key.code).await,
            InputMode::Search => self.handle_search_key(key),
            InputMode::AddLink => self.handle_add_link_key(key).await,
            InputMode::AddSubName => self.handle_add_sub_name_key(key),
            InputMode::AddSubUrl { name } => self.handle_add_sub_url_key(key, name).await,
        }
    }

    pub fn handle_paste(&mut self, text: String) {
        if self.input_mode != InputMode::Normal {
            let cleaned = text.replace(['\r', '\n'], "");
            self.input_buffer.push_str(&cleaned);
        }
    }

    async fn handle_normal_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Tab => {
                self.tab = self.tab.next();
                self.refresh_daemon().await;
            }
            KeyCode::BackTab => {
                self.tab = self.tab.prev();
                self.refresh_daemon().await;
            }
            KeyCode::Char('1') => self.tab = Tab::Servers,
            KeyCode::Char('2') => self.tab = Tab::Subscriptions,
            KeyCode::Char('3') => {
                self.tab = Tab::Logs;
                self.refresh_daemon().await;
            }
            KeyCode::Char('4') => self.tab = Tab::Settings,
            KeyCode::Char(' ') => self.toggle_connection().await,
            KeyCode::Char('m') => self.toggle_mode().await,
            KeyCode::Char('r') => self.toggle_routing().await,
            KeyCode::Char('c') => self.toggle_engine(),
            _ => match self.tab {
                Tab::Servers => self.handle_servers_key(code).await,
                Tab::Subscriptions => self.handle_subs_key(code).await,
                Tab::Logs => self.handle_logs_key(code).await,
                Tab::Settings => self.handle_settings_key(code).await,
            },
        }
    }

    async fn handle_servers_key(&mut self, code: KeyCode) {
        let len = self.filtered_profiles().len();
        match code {
            KeyCode::Down | KeyCode::Char('j') => {
                if len > 0 {
                    self.selected_server = (self.selected_server + 1).min(len - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected_server = self.selected_server.saturating_sub(1);
            }
            KeyCode::Char('g') => self.selected_server = 0,
            KeyCode::Char('G') => {
                if len > 0 {
                    self.selected_server = len - 1;
                }
            }
            KeyCode::Enter => self.connect_selected().await,
            KeyCode::Char('a') => {
                self.input_buffer.clear();
                self.input_mode = InputMode::AddLink;
            }
            KeyCode::Char('i') => self.import_from_clipboard(),
            KeyCode::Char('p') => self.ping_all_profiles().await,
            KeyCode::Char('s') => self.sort_by_latency(),
            KeyCode::Char('d') => self.delete_selected_profile(),
            KeyCode::Char('/') => {
                self.input_buffer = self.search_query.clone();
                self.input_mode = InputMode::Search;
            }
            KeyCode::Esc => {
                if !self.search_query.is_empty() {
                    self.search_query.clear();
                    self.selected_server = 0;
                    self.status_message = "Search filter cleared".to_string();
                }
            }
            _ => {}
        }
    }

    async fn handle_subs_key(&mut self, code: KeyCode) {
        let len = self.subscriptions.len();
        match code {
            KeyCode::Down | KeyCode::Char('j') => {
                if len > 0 {
                    self.selected_sub = (self.selected_sub + 1).min(len - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected_sub = self.selected_sub.saturating_sub(1);
            }
            KeyCode::Char('a') => {
                self.input_buffer.clear();
                self.input_mode = InputMode::AddSubName;
            }
            KeyCode::Char('u') | KeyCode::Enter => self.update_subscriptions().await,
            KeyCode::Char('d') => self.delete_selected_subscription(),
            _ => {}
        }
    }

    async fn handle_logs_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.log_scroll = self.log_scroll.saturating_add(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.log_scroll = self.log_scroll.saturating_sub(1);
            }
            KeyCode::Char('g') => self.log_scroll = 0,
            KeyCode::Char('G') => self.log_scroll = self.logs.len(),
            KeyCode::Char('u') | KeyCode::Enter => self.refresh_daemon().await,
            _ => {}
        }
    }

    async fn handle_settings_key(&mut self, code: KeyCode) {
        const SETTINGS_COUNT: usize = 4;
        match code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected_setting = (self.selected_setting + 1).min(SETTINGS_COUNT - 1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected_setting = self.selected_setting.saturating_sub(1);
            }
            KeyCode::Enter => match self.selected_setting {
                0 => self.toggle_mode().await,
                1 => self.toggle_engine(),
                2 => self.toggle_routing().await,
                3 => {
                    self.settings.ipv6_enabled = !self.settings.ipv6_enabled;
                    self.save_settings_with_msg("IPv6 setting updated");
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('v') {
            if let Ok(text) = clipboard::read_clipboard() {
                self.handle_paste(text);
            }
            return;
        }
        match key.code {
            KeyCode::Enter => {
                self.search_query = self.input_buffer.clone();
                self.selected_server = 0;
                self.input_mode = InputMode::Normal;
                self.status_message = format!("Filter: '{}'", self.search_query);
            }
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
            }
            _ => {}
        }
    }

    async fn handle_add_link_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('v') {
            if let Ok(text) = clipboard::read_clipboard() {
                self.handle_paste(text);
            }
            return;
        }
        match key.code {
            KeyCode::Enter => {
                let raw = self.input_buffer.trim().to_string();
                self.input_buffer.clear();
                self.input_mode = InputMode::Normal;
                if raw.is_empty() {
                    self.status_message = "Empty input".to_string();
                    return;
                }
                self.import_manual_link(&raw).await;
            }
            KeyCode::Esc => {
                self.input_buffer.clear();
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
            }
            _ => {}
        }
    }

    async fn import_manual_link(&mut self, raw: &str) {
        if let Some(sub_url) = subscription::extract_subscription_url(raw) {
            let name = sub_url
                .split("://")
                .nth(1)
                .and_then(|s| s.split('/').next())
                .unwrap_or("Subscription")
                .to_string();
            let mut sub = Subscription::new(name.clone(), sub_url);
            self.status_message = format!("Fetching subscription '{name}'...");
            match subscription::fetch_subscription(&sub).await {
                Ok(new_profiles) => {
                    if new_profiles.is_empty() {
                        self.status_message =
                            "Subscription fetched, but no supported VPN URIs were found".to_string();
                        return;
                    }
                    let count = new_profiles.len();
                    sub.last_updated = Some(Utc::now());
                    self.subscriptions.push(sub);
                    let first_idx = self.profiles.len();
                    self.profiles.extend(new_profiles);
                    let _ = self.storage.save_subscriptions(&self.subscriptions);
                    let _ = self.storage.save_profiles(&self.profiles);
                    self.search_query.clear();
                    self.selected_server = first_idx;
                    self.status_message =
                        format!("Loaded {count} server(s) from '{name}'. Press Enter to connect.");
                }
                Err(err) => {
                    self.status_message = format!("Subscription error: {err}");
                }
            }
            return;
        }

        let imported = parser::parse_batch(raw);
        if imported.is_empty() {
            self.status_message =
                "Unsupported link. Paste vless://, vmess://, trojan://, ss://, hy2://, happ://add/... or https:// sub".to_string();
            return;
        }
        let count = imported.len();
        let first_idx = self.profiles.len();
        self.profiles.extend(imported);
        if let Err(err) = self.storage.save_profiles(&self.profiles) {
            self.status_message = format!("Failed to save profile: {err}");
        } else {
            self.search_query.clear();
            self.selected_server = first_idx;
            self.status_message =
                format!("Added {count} server(s). Press Enter to connect.");
        }
    }

    fn handle_add_sub_name_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('v') {
            if let Ok(text) = clipboard::read_clipboard() {
                self.handle_paste(text);
            }
            return;
        }
        match key.code {
            KeyCode::Enter => {
                let name = self.input_buffer.trim().to_string();
                if name.is_empty() {
                    self.status_message = "Subscription name cannot be empty".to_string();
                    self.input_mode = InputMode::Normal;
                    return;
                }
                self.input_buffer.clear();
                self.input_mode = InputMode::AddSubUrl { name };
            }
            KeyCode::Esc => self.input_mode = InputMode::Normal,
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(c) => self.input_buffer.push(c),
            _ => {}
        }
    }

    async fn handle_add_sub_url_key(&mut self, key: KeyEvent, name: String) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('v') {
            if let Ok(text) = clipboard::read_clipboard() {
                self.handle_paste(text);
            }
            return;
        }
        match key.code {
            KeyCode::Enter => {
                let raw_url = self.input_buffer.trim().to_string();
                self.input_buffer.clear();
                self.input_mode = InputMode::Normal;
                let Some(url) = subscription::extract_subscription_url(&raw_url) else {
                    self.status_message =
                        "URL must be http(s):// or happ://add/https://...".to_string();
                    return;
                };
                self.subscriptions.push(Subscription::new(name.clone(), url));
                if let Err(err) = self.storage.save_subscriptions(&self.subscriptions) {
                    self.status_message = format!("Failed to save subscription: {err}");
                } else {
                    self.update_subscriptions().await;
                }
            }
            KeyCode::Esc => self.input_mode = InputMode::Normal,
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(c) => self.input_buffer.push(c),
            _ => {}
        }
    }

    async fn connect_selected(&mut self) {
        let maybe_profile = {
            let filtered = self.filtered_profiles();
            filtered
                .get(self.selected_server)
                .map(|(_, p)| (*p).clone())
        };

        let Some(profile) = maybe_profile else {
            self.status_message = "No server selected".to_string();
            return;
        };

        self.settings.selected_profile_id = Some(profile.id.clone());
        let _ = self.storage.save_settings(&self.settings);

        self.status_message = format!("Connecting to {}...", profile.display_name());
        match self.client.start(&profile, &self.settings).await {
            Ok(st) => {
                self.daemon_status = st;
                self.daemon_reachable = true;
                self.status_message = format!(
                    "Connected: {} ({})",
                    profile.display_name(),
                    self.settings.mode.as_str()
                );
            }
            Err(err) => {
                self.status_message = format!("Connect failed: {err}");
            }
        }
    }

    async fn toggle_connection(&mut self) {
        if self.daemon_status.running {
            match self.client.stop().await {
                Ok(st) => {
                    self.daemon_status = st;
                    self.daemon_reachable = true;
                    self.status_message = "Disconnected".to_string();
                }
                Err(err) => {
                    self.status_message = format!("Disconnect failed: {err}");
                }
            }
        } else {
            self.connect_selected().await;
        }
    }

    async fn toggle_mode(&mut self) {
        self.settings.mode = self.settings.mode.toggle();
        self.save_settings_with_msg(&format!("Mode: {}", self.settings.mode.as_str()));
    }

    async fn toggle_routing(&mut self) {
        self.settings.routing = self.settings.routing.toggle();
        self.save_settings_with_msg(&format!("Routing: {}", self.settings.routing.as_str()));
    }

    fn toggle_engine(&mut self) {
        self.settings.engine = self.settings.engine.next();
        self.save_settings_with_msg(&format!("Core Engine: {}", self.settings.engine.as_str()));
    }

    fn save_settings_with_msg(&mut self, msg: &str) {
        if let Err(err) = self.storage.save_settings(&self.settings) {
            self.status_message = format!("Failed to save settings: {err}");
        } else {
            self.status_message = msg.to_string();
        }
    }

    fn import_from_clipboard(&mut self) {
        match clipboard::read_clipboard() {
            Ok(text) => {
                let imported = parser::parse_batch(&text);
                if imported.is_empty() {
                    self.status_message = "No supported VPN URIs found in clipboard".to_string();
                    return;
                }
                let count = imported.len();
                self.profiles.extend(imported);
                match self.storage.save_profiles(&self.profiles) {
                    Ok(()) => {
                        self.status_message = format!("Imported {count} profile(s) from clipboard");
                    }
                    Err(err) => {
                        self.status_message = format!("Failed to save profiles: {err}");
                    }
                }
            }
            Err(err) => {
                self.status_message = format!("Clipboard error: {err}");
            }
        }
    }

    async fn ping_all_profiles(&mut self) {
        if self.profiles.is_empty() {
            self.status_message = "No profiles to ping".to_string();
            return;
        }
        self.status_message = format!("Pinging {} servers...", self.profiles.len());
        let results = ping::ping_all(&self.profiles, 16, 2000).await;
        for (id, ms) in results {
            if let Some(p) = self.profiles.iter_mut().find(|x| x.id == id) {
                p.latency_ms = Some(ms);
            }
        }
        let _ = self.storage.save_profiles(&self.profiles);
        self.status_message = "Ping complete (press 's' to sort by latency)".to_string();
    }

    fn sort_by_latency(&mut self) {
        self.profiles.sort_by_key(|p| match p.latency_ms {
            Some(ms) if ms >= 0 => ms,
            _ => i64::MAX,
        });
        let _ = self.storage.save_profiles(&self.profiles);
        self.selected_server = 0;
        self.status_message = "Sorted servers by latency".to_string();
    }

    fn delete_selected_profile(&mut self) {
        let maybe_idx = self
            .filtered_profiles()
            .get(self.selected_server)
            .map(|(idx, _)| *idx);

        if let Some(idx) = maybe_idx {
            let removed = self.profiles.remove(idx);
            let _ = self.storage.save_profiles(&self.profiles);
            let new_len = self.filtered_profiles().len();
            if self.selected_server >= new_len && new_len > 0 {
                self.selected_server = new_len - 1;
            }
            self.status_message = format!("Deleted '{}'", removed.display_name());
        }
    }

    fn delete_selected_subscription(&mut self) {
        if self.selected_sub < self.subscriptions.len() {
            let sub = self.subscriptions.remove(self.selected_sub);
            self.profiles
                .retain(|p| p.subscription_id.as_deref() != Some(sub.id.as_str()));
            let _ = self.storage.save_subscriptions(&self.subscriptions);
            let _ = self.storage.save_profiles(&self.profiles);
            if self.selected_sub >= self.subscriptions.len() && !self.subscriptions.is_empty() {
                self.selected_sub = self.subscriptions.len() - 1;
            }
            self.status_message = format!("Deleted subscription '{}'", sub.name);
        }
    }

    async fn update_subscriptions(&mut self) {
        if self.subscriptions.is_empty() {
            self.status_message = "No subscriptions configured (press 'a' to add)".to_string();
            return;
        }
        let mut total_imported = 0usize;
        let mut errors = Vec::new();

        for sub in &mut self.subscriptions {
            if !sub.enabled {
                continue;
            }
            match subscription::fetch_subscription(sub).await {
                Ok(new_profiles) => {
                    self.profiles
                        .retain(|p| p.subscription_id.as_deref() != Some(sub.id.as_str()));
                    total_imported += new_profiles.len();
                    self.profiles.extend(new_profiles);
                    sub.last_updated = Some(Utc::now());
                }
                Err(err) => {
                    errors.push(format!("{}: {err}", sub.name));
                }
            }
        }

        let _ = self.storage.save_subscriptions(&self.subscriptions);
        let _ = self.storage.save_profiles(&self.profiles);

        if errors.is_empty() {
            self.status_message = format!("Subscriptions updated: {total_imported} servers loaded");
        } else {
            self.status_message = format!("Updated ({total_imported} ok), errors: {}", errors.join("; "));
        }
    }
}
