use std::{sync::Arc, time::Instant};

use gpui_kit::{AppContext as _, Context, Entity, SharedString, Window};

use crate::components::source_menu::{LoadMethod, LoadTarget, open_manual_dialog, open_url_dialog};
use crate::document::{LoadedDocument, SourceKind};
use crate::error::AppError;
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
        self.apply_load((), LoadMethod::Clipboard, window, cx);
    }

    /// Success notes stay on the home page's status line; failures pop up as
    /// notifications so they cannot be missed.
    fn note_home(&self, ok: bool, text: impl Into<String>, cx: &mut Context<Self>) {
        let Some(text) = crate::components::chrome::inline_success_or_notify_failure(ok, text, cx)
        else {
            return;
        };
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
    pub fn open_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_load((), LoadMethod::PickFile, window, cx);
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
        self.apply_load((), LoadMethod::Sample, window, cx);
    }

    pub fn open_doc(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_load((), LoadMethod::History(id), window, cx);
    }
}

impl LoadTarget for HJsonApp {
    type Slot = ();

    fn history_app(&self) -> Option<&gpui_kit::WeakEntity<HJsonApp>> {
        Some(&self.me)
    }

    fn load_started(&mut self, _slot: (), method: &LoadMethod, cx: &mut Context<Self>) {
        if let LoadMethod::FetchUrl { url, .. } = method {
            self.note_home(
                true,
                tr!("Requesting %{url} …", url = url.clone()).to_string(),
                cx,
            );
        }
    }

    fn load_succeeded(
        &mut self,
        _slot: (),
        method: LoadMethod,
        document: LoadedDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let loaded_url = document.url.clone().unwrap_or_default();
        self.open_document(document, window, cx);
        match method {
            LoadMethod::FetchUrl { .. } => {
                self.note_home(true, tr!("Loaded %{url}", url = loaded_url).to_string(), cx)
            }
            LoadMethod::PickFile => self.note_home(true, tr!("File opened"), cx),
            _ => {}
        }
    }

    fn load_failed(&mut self, _slot: (), error: AppError, cx: &mut Context<Self>) {
        self.note_home(false, error.message(), cx);
    }
}

fn document_preview(text: &str) -> String {
    text.replace(['\n', '\r', '\t'], " ")
        .chars()
        .take(96)
        .collect()
}
