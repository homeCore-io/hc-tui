//! Manage › Status: `GET /system/status`.

use crate::app::App;

#[derive(Debug, Default)]
pub struct StatusPanel {
    pub loading: bool,
    pub last_refresh: Option<String>,
    pub error: Option<String>,
}

impl App {
    pub(crate) fn refresh_system_status(&mut self) {
        self.manage.status.loading = true;
        let c = self.client.clone();
        self.spawn(async move { c.get_system_status().await }, |app, res| {
            let p = &mut app.manage.status;
            p.loading = false;
            match res {
                Ok(s) => {
                    p.error = None;
                    p.last_refresh = Some(chrono::Local::now().format("%H:%M:%S").to_string());
                    app.set_system_status(s);
                    app.status = "System status refreshed".to_string();
                }
                Err(e) => {
                    p.error = Some(e.to_string());
                    app.error = Some(format!("Failed to load system status: {e}"));
                }
            }
        });
    }
}
