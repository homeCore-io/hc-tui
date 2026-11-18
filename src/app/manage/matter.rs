//! Plugins › Matter: commissioned nodes from `GET /plugins/matter/nodes`.

use crate::app::editors::{Confirm, ConfirmAction, MatterCommissionEditor};
use crate::app::{App, ListSel};
use crate::model::MatterNode;
use crossterm::event::{KeyCode, KeyEvent};
use serde_json::{Map, Value, json};
use std::collections::VecDeque;

#[derive(Debug)]
pub struct MatterPanel {
    pub sel: ListSel,
    pub editor: Option<MatterCommissionEditor>,
    pub loading: bool,
    /// A commission was accepted and the node count has not grown yet.
    pub pending: bool,
    pub last_action: String,
    pub activity: VecDeque<String>,
}

impl Default for MatterPanel {
    fn default() -> Self {
        Self {
            sel: ListSel::default(),
            editor: None,
            loading: false,
            pending: false,
            last_action: "No Matter operation started".to_string(),
            activity: VecDeque::new(),
        }
    }
}

impl MatterPanel {
    fn log(&mut self, line: String) {
        let ts = chrono::Local::now().format("%H:%M:%S");
        self.activity.push_front(format!("[{ts}] {line}"));
        self.activity.truncate(8);
        self.last_action = line;
    }
}

impl App {
    pub fn selected_matter_node(&self) -> Option<&MatterNode> {
        match self.manage.matter.sel.id() {
            Some(id) => self.matter_nodes.iter().find(|n| n.node_id == id),
            None => self.matter_nodes.get(self.manage.matter.sel.index()),
        }
    }

    pub(crate) fn on_key_matter(&mut self, key: KeyEvent) {
        let ids: Vec<String> = self
            .matter_nodes
            .iter()
            .map(|n| n.node_id.clone())
            .collect();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.manage.matter.sel.move_by(1, &ids),
            KeyCode::Up | KeyCode::Char('k') => self.manage.matter.sel.move_by(-1, &ids),
            KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Char('n') => {
                self.manage.matter.editor = Some(MatterCommissionEditor::default());
            }
            KeyCode::Enter | KeyCode::Char('i') => self.reinterview_selected_node(),
            KeyCode::Char('x') | KeyCode::Delete => {
                if let Some(n) = self.selected_matter_node() {
                    let node_id = n.node_id.clone();
                    let count = n.device_count;
                    self.ask(
                        Confirm::new(
                            "Remove Matter node",
                            format!("Remove Matter node {node_id}?"),
                            ConfirmAction::RemoveMatterNode { node_id },
                        )
                        .with_detail(format!("It has {count} device(s).")),
                    );
                }
            }
            _ => {}
        }
    }

    pub(crate) fn refresh_matter_nodes(&mut self) {
        self.manage.matter.loading = true;
        let c = self.client.clone();
        self.spawn(async move { c.list_matter_nodes().await }, |app, res| {
            app.manage.matter.loading = false;
            match res {
                Ok(nodes) => {
                    let before = app.matter_nodes.len();
                    app.matter_nodes = nodes;
                    let after = app.matter_nodes.len();
                    let m = &mut app.manage.matter;
                    if m.pending && after > before {
                        m.pending = false;
                        m.log(format!("Commission completed: nodes {before} -> {after}"));
                    } else {
                        m.log(format!("Matter nodes refreshed ({after})"));
                    }
                    app.status = app.manage.matter.last_action.clone();
                }
                Err(e) => {
                    let msg = format!("Matter list failed: {e}");
                    app.manage.matter.log(msg.clone());
                    app.error = Some(msg);
                }
            }
        });
    }

    pub(crate) fn save_matter_editor(&mut self) {
        let Some(ed) = self.manage.matter.editor.clone() else {
            return;
        };
        let mut payload = Map::new();
        let mut text = |key: &str, v: &str| {
            let v = v.trim();
            if !v.is_empty() {
                payload.insert(key.to_string(), Value::String(v.to_string()));
            }
        };
        text("pairing_code", &ed.pairing_code);
        text("name", &ed.name);
        text("area", &ed.room);
        if !ed.discriminator.trim().is_empty() {
            match ed.discriminator.trim().parse::<u16>() {
                Ok(v) => {
                    payload.insert("discriminator".into(), json!(v));
                }
                Err(_) => {
                    self.error = Some("Matter discriminator must be a number (0-65535)".into());
                    return;
                }
            }
        }
        if !ed.passcode.trim().is_empty() {
            match ed.passcode.trim().parse::<u32>() {
                Ok(v) => {
                    payload.insert("passcode".into(), json!(v));
                }
                Err(_) => {
                    self.error = Some("Matter passcode must be a number".into());
                    return;
                }
            }
        }
        self.manage.matter.editor = None;
        let c = self.client.clone();
        self.spawn(
            async move { c.matter_commission(Value::Object(payload)).await },
            |app, res| match res {
                Ok(()) => {
                    app.manage.matter.pending = true;
                    app.manage
                        .matter
                        .log("Commission request accepted; waiting for the device".into());
                    app.status = app.manage.matter.last_action.clone();
                    app.refresh_matter_nodes();
                }
                Err(e) => {
                    app.manage.matter.pending = false;
                    let msg = format!("Matter commission failed: {e}");
                    app.manage.matter.log(msg.clone());
                    app.error = Some(msg);
                }
            },
        );
    }

    fn reinterview_selected_node(&mut self) {
        let Some(node_id) = self.selected_matter_node().map(|n| n.node_id.clone()) else {
            self.error = Some("No Matter node selected".to_string());
            return;
        };
        let c = self.client.clone();
        let target = node_id.clone();
        self.spawn(
            async move { c.matter_reinterview(&target).await },
            move |app, res| match res {
                Ok(()) => {
                    app.manage
                        .matter
                        .log(format!("Reinterview requested for {node_id}"));
                    app.status = app.manage.matter.last_action.clone();
                    app.refresh_matter_nodes();
                }
                Err(e) => {
                    let msg = format!("Matter reinterview failed for {node_id}: {e}");
                    app.manage.matter.log(msg.clone());
                    app.error = Some(msg);
                }
            },
        );
    }

    pub(crate) fn remove_matter_node(&mut self, node_id: String) {
        let c = self.client.clone();
        let target = node_id.clone();
        self.spawn(
            async move { c.matter_remove_node(&target).await },
            move |app, res| match res {
                Ok(()) => {
                    app.manage
                        .matter
                        .log(format!("Removal requested for {node_id}"));
                    app.status = app.manage.matter.last_action.clone();
                    app.refresh_matter_nodes();
                }
                Err(e) => {
                    let msg = format!("Matter remove failed for {node_id}: {e}");
                    app.manage.matter.log(msg.clone());
                    app.error = Some(msg);
                }
            },
        );
    }
}
