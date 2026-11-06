use gpui_kit::{Context, WeakEntity, Window};

use crate::components::source_menu::{LoadMethod, LoadTarget};
use crate::document::LoadedDocument;
use crate::error::{AppError, Status};
use crate::tr;

use super::{DiffPhase, DiffView, SideIx};

impl LoadTarget for DiffView {
    type Slot = SideIx;

    fn history_app(&self) -> Option<&WeakEntity<crate::app::HJsonApp>> {
        Some(&self.app)
    }

    fn refetch_url(&self, side: SideIx) -> Option<(String, Option<String>)> {
        let (url, title) = match &self.phase {
            DiffPhase::Config(cfg) => (
                cfg.sides[side].url.clone(),
                cfg.sides[side].title.as_ref().map(ToString::to_string),
            ),
            DiffPhase::Result(res) => (
                res.sources[side].url.clone(),
                Some(res.sources[side].title.to_string()),
            ),
        };
        url.map(|url| (url, title))
    }

    fn load_started(&mut self, side: SideIx, method: &LoadMethod, cx: &mut Context<Self>) {
        if let LoadMethod::FetchUrl { url, .. } = method
            && let DiffPhase::Config(cfg) = &mut self.phase
        {
            cfg.sides[side].busy = true;
            cfg.sides[side].note = Some(Status::success(
                tr!("Requesting %{url} …", url = url.clone()).to_string(),
            ));
            cx.notify();
        }
    }

    fn load_succeeded(
        &mut self,
        side: SideIx,
        method: LoadMethod,
        document: LoadedDocument,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = method;
        self.fill_source(side, document.into(), cx);
    }

    fn load_failed(&mut self, side: SideIx, error: AppError, cx: &mut Context<Self>) {
        self.note_side(side, false, error.message(), cx);
    }
}
