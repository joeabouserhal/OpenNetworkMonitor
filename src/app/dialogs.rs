//! File pickers that run beside the window instead of inside the UI thread.
//!
//! A native dialog blocks the thread that opens it. Opened from the UI thread it froze
//! drawing, tray commands and a newer build's request to take over until it closed.
use eframe::egui;
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    ExportCsv,
    CountryDatabase,
}

#[derive(Default)]
pub struct FileDialogs {
    open: Option<(Purpose, Receiver<Option<PathBuf>>)>,
}
impl FileDialogs {
    /// One dialog at a time; the buttons that open them are disabled meanwhile.
    pub fn busy(&self) -> bool {
        self.open.is_some()
    }
    pub fn save_csv(&mut self, ctx: &egui::Context, name: String) {
        self.spawn(ctx, Purpose::ExportCsv, move || {
            rfd::FileDialog::new()
                .add_filter("CSV", &["csv"])
                .set_file_name(name)
                .save_file()
        });
    }
    pub fn pick_country_database(&mut self, ctx: &egui::Context) {
        self.spawn(ctx, Purpose::CountryDatabase, || {
            rfd::FileDialog::new()
                .add_filter("MaxMind Country or City database", &["mmdb"])
                .pick_file()
        });
    }
    fn spawn(
        &mut self,
        ctx: &egui::Context,
        purpose: Purpose,
        pick: impl FnOnce() -> Option<PathBuf> + Send + 'static,
    ) {
        if self.open.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let started = std::thread::Builder::new()
            .name("file-dialog".into())
            .spawn(move || {
                let _ = tx.send(pick());
                ctx.request_repaint();
            });
        if started.is_ok() {
            self.open = Some((purpose, rx));
        }
    }
    /// The file the user chose, once, after the dialog closes. Cancelling gives nothing.
    pub fn poll(&mut self) -> Option<(Purpose, PathBuf)> {
        let (purpose, rx) = self.open.as_ref()?;
        match rx.try_recv() {
            Ok(choice) => {
                let purpose = *purpose;
                self.open = None;
                choice.map(|path| (purpose, path))
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.open = None;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn a_choice_arrives_once_and_cancelling_gives_nothing() {
        let ctx = egui::Context::default();
        let wait = |dialogs: &mut FileDialogs| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(found) = dialogs.poll() {
                    return Some(found);
                }
                if !dialogs.busy() || Instant::now() > deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        let mut dialogs = FileDialogs::default();
        dialogs.spawn(&ctx, Purpose::ExportCsv, || Some(PathBuf::from("a.csv")));
        assert!(dialogs.busy());
        dialogs.spawn(&ctx, Purpose::CountryDatabase, || {
            Some(PathBuf::from("ignored.mmdb"))
        });
        assert_eq!(
            wait(&mut dialogs),
            Some((Purpose::ExportCsv, PathBuf::from("a.csv"))),
            "A second request while one is open is ignored"
        );
        assert!(!dialogs.busy());
        assert_eq!(dialogs.poll(), None);

        dialogs.spawn(&ctx, Purpose::CountryDatabase, || None);
        assert_eq!(wait(&mut dialogs), None);
        assert!(!dialogs.busy(), "Cancelling frees the next dialog");
    }
}
