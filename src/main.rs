use std::sync::Arc;

use futures::{StreamExt as _, channel::mpsc};
use reqwest_client::ReqwestClient;

fn main() {
    // Register before the run loop starts: Finder may deliver the initial
    // document-open event before application initialization finishes.
    let (open_tx, mut open_rx) = mpsc::unbounded();
    let application = gpui_kit::application()
        .with_http_client(Arc::new(
            ReqwestClient::user_agent("HJson").expect("failed to initialize HTTP client"),
        ))
        .with_assets(gpui_kit::assets::AllAssets);
    application.on_open_urls(move |urls| {
        let _ = open_tx.unbounded_send(urls);
    });
    application.run(move |cx| {
        gpui_kit::init(cx);
        hjson::app::init(cx);
        hjson::views::settings::init(cx);
        // After settings: the menu bar shows a check mark read from them.
        hjson::app::menu::init(cx);

        // Element inspector overlay (`cmd-alt-i` toggles it). Debug builds
        // only — release never installs the renderer, so the shortcut and
        // the overlay simply do not exist there.
        #[cfg(debug_assertions)]
        gpui_devtools::init(cx);

        hjson::app::open_window(cx);
        hjson::app::open_external_paths(std::env::args_os().skip(1).map(Into::into).collect(), cx);
        cx.spawn(async move |cx| {
            while let Some(urls) = open_rx.next().await {
                cx.update(|cx| hjson::app::open_external_urls(urls, cx));
            }
        })
        .detach();
    });
}
