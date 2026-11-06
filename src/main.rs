use std::sync::Arc;

use gpui_kit::{AppContext as _, Bounds, WindowBounds, WindowOptions, px, size};
use reqwest_client::ReqwestClient;

fn main() {
    gpui_kit::application()
        .with_http_client(Arc::new(
            ReqwestClient::user_agent("HJson").expect("failed to initialize HTTP client"),
        ))
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
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

            let bounds = Bounds::centered(None, size(px(1280.), px(860.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // Below this the three-pane diff page stops being readable, so
                // macOS refuses to shrink the window any further.
                window_min_size: Some(size(px(900.), px(600.))),
                ..Default::default()
            };

            gpui_kit::open_window(options, cx, |window, cx| {
                window.set_window_title("HJson — JSON Viewer");
                // Apply the persisted appearance (follow OS, or force light /
                // dark), then keep it in step with the system when asked to.
                //
                // The subscription MUST be detached: a `Subscription` is
                // cancelled when dropped, and this closure returns right after
                // opening the window, which would silently remove the observer.
                // While an appearance is forced the re-apply just re-asserts it.
                hjson::views::settings::apply_appearance(Some(window), cx);
                window
                    .observe_window_appearance(|window, cx| {
                        hjson::views::settings::apply_appearance(Some(window), cx);
                    })
                    .detach();
                cx.new(|cx| hjson::app::HJsonApp::new(window, cx))
            })
            .expect("failed to open window");
        });
}
