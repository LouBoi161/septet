//! Earlier conversations, to continue them (`claude --resume`). Claude Code keeps the transcripts;
//! Septet only remembers ids and titles.

use std::path::PathBuf;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Past {
    pub id: String,
    pub title: String,
    /// Seconds since the Unix epoch.
    pub updated: u64,
}

#[derive(Default)]
pub struct History {
    pub items: Vec<Past>,
}

const MAX: usize = 50;

fn file() -> Option<PathBuf> {
    crate::recent::config_dir().map(|d| d.join("assistant").join("conversations.json"))
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl History {
    pub fn load() -> Self {
        let items = file().and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        History { items }
    }

    /// Note that conversation `id` was used; the first message becomes its title.
    pub fn touch(&mut self, id: &str, first_message: &str) {
        match self.items.iter().position(|p| p.id == id) {
            Some(i) => {
                let mut p = self.items.remove(i);
                p.updated = now();
                self.items.insert(0, p);
            }
            None => {
                let mut title: String = first_message.split_whitespace().collect::<Vec<_>>().join(" ");
                if title.chars().count() > 60 {
                    title = title.chars().take(57).collect::<String>() + "…";
                }
                self.items.insert(0, Past { id: id.to_owned(), title, updated: now() });
                self.items.truncate(MAX);
            }
        }
        self.save();
    }

    fn save(&self) {
        let Some(f) = file() else { return };
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(json) = serde_json::to_vec_pretty(&self.items) {
            let _ = std::fs::write(f, json);
        }
    }
}
