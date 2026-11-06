use std::{sync::Arc, time::Instant};

use gpui_kit::{AppContext as _, Context, Entity, SharedString, Window};

use crate::components::source_menu::{open_manual_dialog, open_url_dialog};
use crate::document::SourceKind;
use crate::document::{self, LoadedDocument};
use crate::json_model::JsonTree;
use crate::tr;
use crate::views::doc::DocView;

use super::{HJsonApp, TabPage};

#[derive(Clone)]
pub struct DocEntry {
    pub id: u64,
    pub title: SharedString,
    pub kind: SourceKind,
    /// Original URL (only when the source was fetched over HTTP), kept so a
    /// document can be re-fetched from its toolbar.
    pub url: Option<String>,
    pub text: String,
    pub parsed: std::sync::Weak<JsonTree>,
    /// Number of bytes of the raw JSON text.
    pub size: usize,
    /// One-line preview of the raw JSON for the history list.
    pub preview: String,
    /// When the document was imported this session (for relative timestamps).
    pub opened_at: Instant,
}

struct RecordedDocument {
    id: u64,
    title: SharedString,
    kind: SourceKind,
    url: Option<String>,
    parsed: Arc<JsonTree>,
}

impl HJsonApp {
    fn record_loaded_document(&mut self, document: LoadedDocument) -> RecordedDocument {
        let LoadedDocument {
            kind,
            title,
            text,
            url,
            parsed,
        } = document;

        self.running_doc_id += 1;
        let id = self.running_doc_id;
        let title: SharedString = title.into();
        let view = RecordedDocument {
            id,
            title: title.clone(),
            kind,
            url: url.clone(),
            parsed: parsed.clone(),
        };
        let entry = DocEntry {
            id,
            title,
            kind,
            url,
            size: text.len(),
            preview: document_preview(&text),
            text,
            parsed: Arc::downgrade(&parsed),
            opened_at: Instant::now(),
        };

        self.docs.insert(0, entry);
        if self.docs.len() > 50 {
            self.docs.pop();
        }
        view
    }

    /// Open a loaded document in the active home tab or a new adjacent tab.
    pub(crate) fn open_document(
        &mut self,
        document: LoadedDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> u64 {
        let document = self.record_loaded_document(document);

        let weak = cx.weak_entity();
        let doc_view = cx.new(|cx| {
            DocView::new(
                weak,
                &document.title,
                document.kind,
                document.url,
                document.parsed,
                window,
                cx,
            )
        });

        if matches!(self.tabs.get(self.active), Some(TabPage::Home(_))) {
            // The import page hands itself over to the parsed document.
            self.tabs[self.active] = TabPage::Doc(doc_view);
        } else {
            // A document is being viewed: open the new one in a separate tab.
            self.tabs.insert(self.active + 1, TabPage::Doc(doc_view));
            self.active += 1;
        }
        cx.notify();
        document.id
    }

    /// Replace one document tab while preserving its position.
    pub(crate) fn replace_document_in_view(
        &mut self,
        doc_view: &Entity<DocView>,
        document: LoadedDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<u64, String> {
        let Some(ix) = self
            .tabs
            .iter()
            .position(|page| matches!(page, TabPage::Doc(v) if v == doc_view))
        else {
            return Err(tr!("Tab closed").to_string());
        };
        let document = self.record_loaded_document(document);

        let weak = cx.weak_entity();
        let new_doc_view = cx.new(|cx| {
            DocView::new(
                weak,
                &document.title,
                document.kind,
                document.url,
                document.parsed,
                window,
                cx,
            )
        });
        self.tabs[ix] = TabPage::Doc(new_doc_view);
        cx.notify();
        Ok(document.id)
    }

    pub fn open_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = cx.read_from_clipboard().and_then(|item| item.text());
        match text {
            Some(text) if !text.trim().is_empty() => match document::from_clipboard(text) {
                Ok(document) => {
                    self.open_document(document, window, cx);
                }
                Err(err) => self.note_home(false, err.message(), cx),
            },
            Some(_) => self.note_home(false, tr!("The clipboard is empty"), cx),
            None => self.note_home(false, tr!("No text on the clipboard"), cx),
        }
        cx.notify();
    }

    /// Success notes stay on the home page's status line; failures pop up as
    /// notifications so they cannot be missed.
    fn note_home(&self, ok: bool, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        if !ok {
            crate::components::chrome::notify_failure(text, cx);
            return;
        }
        let Some(TabPage::Home(home)) = self.tabs.get(self.active) else {
            return;
        };
        let home = home.clone();
        home.update(cx, |home, cx| {
            home.set_status(ok, text, cx);
            cx.notify();
        });
    }

    /// Open a file picked from disk.
    pub fn open_file(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let path = match receiver.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let Some(path) = path else { return };
            let read = cx.background_spawn(async move { document::from_file(path) });
            let result = read.await;
            this.update_in(cx, |app, window, cx| {
                match result {
                    Ok(document) => {
                        app.open_document(document, window, cx);
                    }
                    Err(e) => app.note_home(false, e.message(), cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn open_url_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(TabPage::Home(home)) = self.tabs.get(self.active) else {
            return;
        };
        let target = home.downgrade();
        open_url_dialog(&target, (), None, window, cx);
    }

    pub fn open_manual_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(TabPage::Home(home)) = self.tabs.get(self.active) else {
            return;
        };
        let target = home.downgrade();
        open_manual_dialog(&target, (), window, cx);
    }

    pub fn open_sample(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match document::from_sample() {
            Ok(document) => {
                self.open_document(document, window, cx);
            }
            Err(err) => self.note_home(false, err.message(), cx),
        }
        cx.notify();
    }

    pub fn open_doc(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.docs.iter().find(|d| d.id == id) else {
            return;
        };
        let kind = entry.kind;
        let title = entry.title.clone();
        let text = entry.text.clone();
        let url = entry.url.clone();
        match document::from_history(kind, title.to_string(), text, url, entry.parsed.upgrade()) {
            Ok(document) => {
                self.open_document(document, window, cx);
            }
            Err(err) => self.note_home(false, err.message(), cx),
        }
    }
}

fn document_preview(text: &str) -> String {
    text.replace(['\n', '\r', '\t'], " ")
        .chars()
        .take(96)
        .collect()
}
