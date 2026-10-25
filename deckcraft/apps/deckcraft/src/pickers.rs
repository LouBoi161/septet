//! File pickers for an embedding host that don't block its window.
//!
//! DeckCraft asks for a file with a blocking `rfd::FileDialog` on the UI thread. In a host
//! (Septet), that froze every tab while the dialog was open, and the desktop then offered to
//! force-quit the "not responding" window. Here a picker asked for by an action that can run again
//! (`hosted::current_replay`: Open, Save As, Export, inserting a picture…) answers "cancelled" at
//! once, and the picker is shown as an `rfd::AsyncFileDialog` parented to the host's window, with
//! a thread waiting for it. When it answers, the host runs the action again
//! (`SlideApp::replay_pick`) and the picker answers at once with the picked file. Pickers asked
//! for anywhere else still block, as they do standalone.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError};

use deckcraft_ui_egui::hosted::{self, Replay};

/// Which picker: Open (by purpose) or Save (by suggested name).
#[derive(Clone, Debug, PartialEq)]
enum Key {
    Open(String),
    Save(String),
}

#[derive(Default)]
struct State {
    /// A picker an action asked for, to show on the next frame.
    asked: Option<(Key, Replay)>,
    /// A shown picker's answer, still to come.
    waiting: Option<(Receiver<Option<String>>, Key, Replay)>,
    /// The answer, for the action running again.
    ready: Option<(Key, String)>,
}

/// The pickers' shared state: the hooks ask, the host's frame shows and collects.
#[derive(Clone, Default)]
pub struct Pickers(Rc<RefCell<State>>);

/// A file picker hook (`Services::pick_open`, `pick_save`).
pub type PickFn = Box<dyn FnMut(&str) -> Option<String>>;

impl Pickers {
    /// The Open and Save hooks.
    pub fn hooks(&self) -> (PickFn, PickFn) {
        let open = self.clone();
        let save = self.clone();
        (Box::new(move |purpose: &str| open.pick(Key::Open(purpose.to_string()))), Box::new(move |name: &str| save.pick(Key::Save(name.to_string()))))
    }

    fn pick(&self, key: Key) -> Option<String> {
        if hosted::refuse_dialog() {
            return None;
        }
        {
            let mut st = self.0.borrow_mut();
            if st.ready.as_ref().is_some_and(|(k, _)| *k == key) {
                return st.ready.take().map(|(_, path)| path);
            }
            if st.asked.is_some() || st.waiting.is_some() {
                return None;
            }
            if let Some(replay) = hosted::current_replay() {
                st.asked = Some((key, replay));
                return None;
            }
        }
        // Asked for where nothing can run again: the blocking picker, as standalone.
        match key {
            Key::Open(purpose) => with_open_filters(rfd::FileDialog::new(), &purpose, |d, name, exts| d.add_filter(name, exts)).pick_file(),
            Key::Save(name) => rfd::FileDialog::new().set_file_name(name).save_file(),
        }
        .map(|p| p.to_string_lossy().to_string())
    }

    /// Show the picker an action asked for, parented to the host's window, and wait for it off the
    /// UI thread. Call every frame.
    pub fn show(&self, parent: &eframe::Frame, ctx: &egui::Context) {
        let Some((key, replay)) = self.0.borrow_mut().asked.take() else { return };
        let dialog = rfd::AsyncFileDialog::new().set_parent(parent);
        let picking = match key.clone() {
            Key::Open(purpose) => {
                let d = with_open_filters(dialog, &purpose, |d, name, exts| d.add_filter(name, exts));
                Box::pin(d.pick_file()) as std::pin::Pin<Box<dyn std::future::Future<Output = Option<rfd::FileHandle>> + Send>>
            }
            Key::Save(name) => Box::pin(dialog.set_file_name(name).save_file()),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let repaint = ctx.clone();
        let spawned = std::thread::Builder::new().name("file dialog".into()).spawn(move || {
            let picked = pollster::block_on(picking).map(|f| f.path().to_string_lossy().to_string());
            let _ = tx.send(picked);
            repaint.request_repaint();
        });
        match spawned {
            Ok(_) => self.0.borrow_mut().waiting = Some((rx, key, replay)),
            Err(e) => log::error!("deckcraft: couldn't wait for the file dialog: {e}"),
        }
    }

    /// The action to run again, once its picker answered with a file (a cancel ends it).
    pub fn answered(&self) -> Option<Replay> {
        let mut st = self.0.borrow_mut();
        let (rx, _, _) = st.waiting.as_ref()?;
        let picked = match rx.try_recv() {
            Ok(picked) => picked,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => None,
        };
        let (_, key, replay) = st.waiting.take()?;
        let path = picked?;
        st.ready = Some((key, path));
        Some(replay)
    }

    /// An answer the action didn't take is dropped.
    pub fn forget_answer(&self) {
        self.0.borrow_mut().ready = None;
    }
}

/// `d` with the file types offered for `purpose` (as `main.rs` offers them).
fn with_open_filters<D>(d: D, purpose: &str, add: impl Fn(D, &str, &[&str]) -> D) -> D {
    match purpose {
        "picture" => add(d, "Pictures", &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "svg"]),
        "audio" => {
            add(d, "Audio", &["wav", "mp3", "m4a", "m4b", "aac", "flac", "ogg", "oga", "opus", "aif", "aiff", "aifc", "caf", "wma", "weba", "mka"])
        }
        "video" => add(d, "Video", &["mp4", "m4v", "mov", "webm", "mkv", "wmv"]),
        "theme" => add(add(d, "Themes and presentations", &["thmx", "pptx", "potx", "ppsx"]), "Office Theme", &["thmx"]),
        _ => {
            let d = add(d, "Presentations", &["deckcraft", "pptx", "potx", "ppsx"]);
            let d = add(d, "DeckCraft", &["deckcraft"]);
            let d = add(d, "PowerPoint", &["pptx", "potx", "ppsx"]);
            add(d, "Outline", &["txt", "md"])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An action that can run again gets "cancelled" at once and its picker is queued; the answer
    /// goes to that action alone, once.
    #[test]
    fn a_replayable_picker_answers_later_through_the_action() {
        let pickers = Pickers::default();
        let (mut open, _) = pickers.hooks();
        let replay = Replay::Run { purpose: "picture".into(), cmd: "insert.picture".into(), extra: serde_json::json!({}) };
        {
            let _r = hosted_replay(replay.clone());
            assert_eq!(open("picture"), None);
        }
        assert!(pickers.0.borrow().asked.as_ref().is_some_and(|(k, r)| *k == Key::Open("picture".into()) && *r == replay));
        // The picker's thread answers.
        let (tx, rx) = std::sync::mpsc::channel();
        let asked = pickers.0.borrow_mut().asked.take();
        if let Some((key, replay)) = asked {
            pickers.0.borrow_mut().waiting = Some((rx, key, replay));
        }
        assert_eq!(pickers.answered(), None, "nothing yet");
        tx.send(Some("/tmp/a.png".into())).unwrap();
        assert_eq!(pickers.answered(), Some(replay.clone()));
        // The action runs again: the picker answers at once, then is spent.
        let _r = hosted_replay(replay);
        assert_eq!(open("picture").as_deref(), Some("/tmp/a.png"));
        assert!(pickers.0.borrow().ready.is_none());
    }

    /// A cancelled picker runs nothing.
    #[test]
    fn a_cancel_runs_nothing() {
        let pickers = Pickers::default();
        let (tx, rx) = std::sync::mpsc::channel();
        pickers.0.borrow_mut().waiting = Some((rx, Key::Save("a.pptx".into()), Replay::SaveAs { format: "pptx".into() }));
        tx.send(None).unwrap();
        assert_eq!(pickers.answered(), None);
        assert!(pickers.0.borrow().waiting.is_none() && pickers.0.borrow().ready.is_none());
    }

    fn hosted_replay(r: Replay) -> hosted::Replayable {
        hosted::Replayable::new(r)
    }
}
