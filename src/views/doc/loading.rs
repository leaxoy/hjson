use gpui_kit::{Context, WeakEntity, Window};

use crate::components::source_menu::{LoadMethod, LoadTarget};
use crate::document::LoadedDocument;
use crate::error::AppError;
use crate::tr;

use super::DocView;

impl LoadTarget for DocView {
    type Slot = ();

    fn history_app(&self) -> Option<&WeakEntity<crate::app::HJsonApp>> {
        Some(&self.app)
    }

    fn refetch_url(&self, _slot: ()) -> Option<(String, Option<String>)> {
        self.url
            .clone()
            .map(|url| (url, Some(self.title.to_string())))
    }

    fn load_started(&mut self, _slot: (), method: &LoadMethod, cx: &mut Context<Self>) {
        if let LoadMethod::FetchUrl { url, .. } = method {
            self.say(
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
        let _ = method;
        self.replace_with(document, window, cx);
    }

    fn load_failed(&mut self, _slot: (), error: AppError, cx: &mut Context<Self>) {
        self.say(false, error.message(), cx);
    }
}
