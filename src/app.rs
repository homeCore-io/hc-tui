use crate::api::{
    Area, DeviceState, EventEntry, HomeCoreClient, PluginRecord, Rule, Scene, UserInfo,
};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::cmp::min;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusField {
    Username,
    Password,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Devices,
    Scenes,
    Areas,
    Automations,
    Events,
    Users,
    Plugins,
}

impl Tab {
    pub fn title(self) -> &'static str {
        match self {
            Self::Devices => "Devices",
            Self::Scenes => "Scenes",
            Self::Areas => "Areas",
            Self::Automations => "Automations",
            Self::Events => "Events",
            Self::Users => "Users",
            Self::Plugins => "Plugins",
        }
    }
}

pub struct App {
    pub client: HomeCoreClient,
    pub username: String,
    pub password: String,
    pub focus: FocusField,
    pub authenticated: bool,
    pub current_user: Option<UserInfo>,
    pub status: String,
    pub error: Option<String>,
    pub should_quit: bool,
    pub tab: usize,
    pub selected: usize,
    pub devices: Vec<DeviceState>,
    pub scenes: Vec<Scene>,
    pub areas: Vec<Area>,
    pub automations: Vec<Rule>,
    pub events: Vec<EventEntry>,
    pub users: Vec<UserInfo>,
    pub plugins: Vec<PluginRecord>,
}

impl App {
    pub fn new(base_url: String) -> Self {
        Self {
            client: HomeCoreClient::new(base_url),
            username: String::new(),
            password: String::new(),
            focus: FocusField::Username,
            authenticated: false,
            current_user: None,
            status: "Enter credentials and press Enter".to_string(),
            error: None,
            should_quit: false,
            tab: 0,
            selected: 0,
            devices: Vec::new(),
            scenes: Vec::new(),
            areas: Vec::new(),
            automations: Vec::new(),
            events: Vec::new(),
            users: Vec::new(),
            plugins: Vec::new(),
        }
    }

    pub fn tabs(&self) -> Vec<Tab> {
        let mut tabs = vec![
            Tab::Devices,
            Tab::Scenes,
            Tab::Areas,
            Tab::Automations,
            Tab::Events,
        ];
        if self.is_admin() {
            tabs.push(Tab::Users);
            tabs.push(Tab::Plugins);
        }
        tabs
    }

    pub fn active_tab(&self) -> Tab {
        let tabs = self.tabs();
        tabs[self.tab.min(tabs.len().saturating_sub(1))]
    }

    pub fn is_admin(&self) -> bool {
        self.current_user
            .as_ref()
            .map(|u| u.role.is_admin())
            .unwrap_or(false)
    }

    pub async fn login(&mut self) {
        self.error = None;
        if self.username.trim().is_empty() || self.password.is_empty() {
            self.error = Some("username and password are required".to_string());
            return;
        }
        match self.client.login(&self.username, &self.password).await {
            Ok(auth) => {
                self.client.set_token(auth.token);
                self.current_user = Some(auth.user);
                self.authenticated = true;
                self.status = "Login successful. Loading data...".to_string();
                if let Err(err) = self.refresh_all().await {
                    self.error = Some(err.to_string());
                }
            }
            Err(err) => {
                self.error = Some(err.to_string());
            }
        }
    }

    pub async fn refresh_all(&mut self) -> Result<()> {
        self.status = "Refreshing...".to_string();
        self.devices = self.client.list_devices().await?;
        self.scenes = self.client.list_scenes().await?;
        self.areas = self.client.list_areas().await?;
        self.automations = self.client.list_automations().await?;
        self.events = self.client.list_events(50).await?;
        if self.is_admin() {
            self.users = self.client.list_users().await?;
            self.plugins = self.client.list_plugins().await?;
        }
        if self.current_user.is_none() {
            self.current_user = Some(self.client.me().await?);
        }
        self.clamp_selection();
        self.status = "Data refreshed".to_string();
        Ok(())
    }

    pub fn on_key_login(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.should_quit = true;
                true
            }
            KeyCode::Tab => {
                self.focus = match self.focus {
                    FocusField::Username => FocusField::Password,
                    FocusField::Password => FocusField::Username,
                };
                false
            }
            KeyCode::BackTab => {
                self.focus = match self.focus {
                    FocusField::Username => FocusField::Password,
                    FocusField::Password => FocusField::Username,
                };
                false
            }
            KeyCode::Backspace => {
                match self.focus {
                    FocusField::Username => {
                        self.username.pop();
                    }
                    FocusField::Password => {
                        self.password.pop();
                    }
                }
                false
            }
            KeyCode::Enter => true,
            KeyCode::Char(ch) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    return false;
                }
                match self.focus {
                    FocusField::Username => self.username.push(ch),
                    FocusField::Password => self.password.push(ch),
                }
                false
            }
            _ => false,
        }
    }

    pub async fn on_key_authenticated(&mut self, key: KeyEvent) {
        self.error = None;
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('r') => {
                if let Err(err) = self.refresh_all().await {
                    self.error = Some(err.to_string());
                }
            }
            KeyCode::Left | KeyCode::BackTab => {
                let tab_count = self.tabs().len();
                self.tab = (self.tab + tab_count - 1) % tab_count;
                self.selected = 0;
                self.clamp_selection();
            }
            KeyCode::Right | KeyCode::Tab => {
                let tab_count = self.tabs().len();
                self.tab = (self.tab + 1) % tab_count;
                self.selected = 0;
                self.clamp_selection();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let len = self.active_items_len();
                if len > 0 {
                    self.selected = min(self.selected + 1, len - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Char('t') => {
                if matches!(self.active_tab(), Tab::Devices) {
                    self.toggle_selected_device().await;
                }
            }
            KeyCode::Char('a') => {
                if matches!(self.active_tab(), Tab::Scenes) {
                    self.activate_selected_scene().await;
                }
            }
            _ => {}
        }
    }

    fn active_items_len(&self) -> usize {
        match self.active_tab() {
            Tab::Devices => self.devices.len(),
            Tab::Scenes => self.scenes.len(),
            Tab::Areas => self.areas.len(),
            Tab::Automations => self.automations.len(),
            Tab::Events => self.events.len(),
            Tab::Users => self.users.len(),
            Tab::Plugins => self.plugins.len(),
        }
    }

    fn clamp_selection(&mut self) {
        let len = self.active_items_len();
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }

    async fn toggle_selected_device(&mut self) {
        let Some(device) = self.devices.get(self.selected) else {
            return;
        };
        let current_on = device
            .attributes
            .get("on")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        match self
            .client
            .set_device_on(&device.device_id, !current_on)
            .await
        {
            Ok(_) => {
                self.status = format!("Set {} to on={}", device.device_id, !current_on);
                if let Err(err) = self.refresh_all().await {
                    self.error = Some(err.to_string());
                }
            }
            Err(err) => {
                self.error = Some(err.to_string());
            }
        }
    }

    async fn activate_selected_scene(&mut self) {
        let Some(scene) = self.scenes.get(self.selected) else {
            return;
        };
        match self.client.activate_scene(&scene.id).await {
            Ok(_) => {
                self.status = format!("Activated scene '{}'", scene.name);
                if let Err(err) = self.refresh_all().await {
                    self.error = Some(err.to_string());
                }
            }
            Err(err) => {
                self.error = Some(err.to_string());
            }
        }
    }
}
