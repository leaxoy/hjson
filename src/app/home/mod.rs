use super::{FetchUrl, HJsonApp, ManualInput, OpenFile, OpenSample, PasteJson};
use crate::components::chrome;
use crate::components::source_menu::{LoadMethod, LoadTarget, open_manual_dialog, open_url_dialog};
use crate::document::{LoadedDocument, SourceKind};
use crate::error::{AppError, Status};
use crate::tr;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Context, Image, ImageFormat, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    WeakEntity, Window, div, img, px,
};
use std::sync::{Arc, OnceLock};

/// One home tab: the OK-JSON-style import page. The documents it opens and the
/// session history both belong to the app, so any tab can hand a document to
/// the sidebar.
pub struct HomeView {
    app: WeakEntity<HJsonApp>,
    status: Option<Status>,
}

impl HomeView {
    pub(super) fn new(app: WeakEntity<HJsonApp>) -> Self {
        Self { app, status: None }
    }

    /// Success notes stay on the page's status line; failures pop up as
    /// notifications so they cannot be missed.
    pub(super) fn set_status(&mut self, ok: bool, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        if ok {
            self.status = Some(Status::from_parts(true, text));
        } else {
            self.status = None;
            chrome::notify_failure(text, cx);
        }
    }

    /// Hand a loaded document to the app: the active home tab turns into the
    /// document.
    fn submit(
        &mut self,
        document: LoadedDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<u64, String> {
        let outcome = match self
            .app
            .update(cx, move |app, cx| app.open_document(document, window, cx))
        {
            Ok(id) => Ok(id),
            Err(err) => Err(err.to_string()),
        };
        match outcome {
            Ok(id) => {
                self.status = None;
                Ok(id)
            }
            Err(err) => {
                self.set_status(false, AppError::parse(err.as_str()).message(), cx);
                Err(err)
            }
        }
    }

    pub fn open_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_load((), LoadMethod::Clipboard, window, cx);
    }

    pub fn open_manual(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_load((), LoadMethod::Manual(text.to_string()), window, cx);
    }

    /// Ask for a URL in a modal, then fetch it. The dialog lives in
    /// [`crate::components::source_menu`] so it is the same one the diff and document view toolbars
    /// open.
    fn open_url_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = cx.entity().downgrade();
        open_url_dialog(&target, (), None, window, cx);
    }

    /// Ask for raw JSON in a modal, then parse it.
    fn open_manual_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = cx.entity().downgrade();
        open_manual_dialog(&target, (), window, cx);
    }

    pub fn open_sample(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_load((), LoadMethod::Sample, window, cx);
    }

    pub fn open_doc(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_load((), LoadMethod::History(id), window, cx);
    }

    pub fn fetch_url(
        &mut self,
        url: String,
        custom_title: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_load(
            (),
            LoadMethod::FetchUrl {
                url,
                title: custom_title,
            },
            window,
            cx,
        );
    }

    pub fn pick_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_load((), LoadMethod::PickFile, window, cx);
    }
}

impl LoadTarget for HomeView {
    type Slot = ();

    fn history_app(&self) -> Option<&WeakEntity<HJsonApp>> {
        Some(&self.app)
    }

    fn load_started(&mut self, _slot: (), method: &LoadMethod, cx: &mut Context<Self>) {
        if let LoadMethod::FetchUrl { url, .. } = method {
            self.status = Some(Status::success(
                tr!("Requesting %{url} …", url = url.clone()).to_string(),
            ));
            cx.notify();
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
        if self.submit(document, window, cx).is_err() {
            return;
        }
        match method {
            LoadMethod::FetchUrl { .. } => {
                self.set_status(true, tr!("Loaded %{url}", url = loaded_url).to_string(), cx);
            }
            LoadMethod::PickFile => self.set_status(true, tr!("File opened"), cx),
            _ => {}
        }
    }

    fn load_failed(&mut self, _slot: (), error: AppError, cx: &mut Context<Self>) {
        self.set_status(false, error.message(), cx);
    }
}

impl Render for HomeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_home(window, cx)
    }
}

impl HomeView {
    /// OK-JSON-style home: the app icon over the centered list of import
    /// actions. History is a sidebar of its own now (`⌘⇧H`), so this page stays
    /// about one thing — getting a document in.
    fn render_home(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        // ---- brand ----
        // The app icon, centered over the import list.
        let header = h_flex()
            .w_full()
            .justify_center()
            .pb_2()
            .child(img(app_icon()).size_24());

        // ---- import actions ----
        // The five ways in, as one centered vertical list of button-links. Every
        // row hands off to the same methods the toolbar's load menu uses, so the
        // two entry points stay in step.
        let import_list = v_flex()
            .w_full()
            .max_w(px(440.))
            .self_center()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .overflow_hidden()
            .child(action_row(
                "paste-clipboard",
                SourceKind::Clipboard,
                tr!("Paste JSON"),
                chrome::kbd_hint(&PasteJson, "⌘⇧V", window, cx),
                cx.listener(|this, _, window, cx| this.open_clipboard(window, cx)),
                cx,
            ))
            .child(Separator::horizontal())
            .child(action_row(
                "open-url",
                SourceKind::Url,
                tr!("Fetch from URL"),
                chrome::kbd_hint(&FetchUrl, "⌘L", window, cx),
                cx.listener(|this, _, window, cx| this.open_url_dialog(window, cx)),
                cx,
            ))
            .child(Separator::horizontal())
            .child(action_row(
                "open-file",
                SourceKind::File,
                tr!("Open file"),
                chrome::kbd_hint(&OpenFile, "⌘O", window, cx),
                cx.listener(|this, _, window, cx| this.pick_file(window, cx)),
                cx,
            ))
            .child(Separator::horizontal())
            .child(action_row(
                "manual-input",
                SourceKind::Manual,
                tr!("Manual input"),
                chrome::kbd_hint(&ManualInput, "⌘I", window, cx),
                cx.listener(|this, _, window, cx| this.open_manual_dialog(window, cx)),
                cx,
            ))
            .child(Separator::horizontal())
            .child(action_row(
                "open-sample",
                SourceKind::Sample,
                tr!("Sample document"),
                chrome::kbd_hint(&OpenSample, "⌘⇧S", window, cx),
                cx.listener(|this, _, window, cx| this.open_sample(window, cx)),
                cx,
            ));

        let status_line = self.status.clone().map(|s| {
            let ok = s.is_success();
            let text = s.message();
            h_flex()
                .gap_2()
                .min_w_0()
                .px_1()
                .text_sm()
                .text_color(if ok { theme.success } else { theme.danger })
                .child(
                    Icon::new(if ok {
                        IconName::CircleCheck
                    } else {
                        IconName::CircleAlert
                    })
                    .small()
                    .flex_shrink_0(),
                )
                .child(div().min_w_0().truncate().child(text))
        });

        // ---- page ----
        // One centered column, so the cards stay aligned with the brand bar
        // instead of hugging the left edge on a wide window.
        let column = v_flex()
            .w_full()
            .max_w(px(880.))
            .gap_4()
            .child(header)
            .child(import_list)
            .children(status_line);

        // Equal spacers above and below center the column in the viewport. They
        // cannot shrink, so once the content is taller than the window they
        // collapse to nothing and the page just scrolls from the top.
        let spacer = || div().w_full().flex_1().flex_shrink_0();

        // `v_flex`, not `div`: a plain div is block layout, where `items_center`
        // is ignored and the capped column would hug the left edge at any width.
        v_flex()
            .id("home")
            .size_full()
            .overflow_y_scroll()
            .items_center()
            .px(px(24.))
            .pt(px(18.))
            .pb(px(24.))
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_base()
            .font_family(theme.font_family.clone())
            .child(spacer())
            .child(column)
            .child(spacer())
    }
}

/// One row of the home page's import list: the source's own glyph in its accent
/// color, its label, and a muted hint of what the click does next.
fn action_row(
    id: &'static str,
    kind: SourceKind,
    label: impl Into<SharedString>,
    hint: AnyElement,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &Context<HomeView>,
) -> AnyElement {
    let label = label.into();
    let theme = cx.theme();
    div()
        .id(id)
        .w_full()
        .px_4()
        .py(px(11.))
        .cursor_pointer()
        .hover(|this| this.bg(theme.list_hover))
        .on_click(on_click)
        .child(
            h_flex()
                .gap_3()
                .items_center()
                .child(Icon::new(kind.icon()).small().text_color(kind.color(cx)))
                .child(div().min_w_0().truncate().child(label))
                .child(h_flex().flex_1())
                .child(hint),
        )
        .into_any_element()
}

/// The app icon (resources/icon.svg), embedded so the home page renders the
/// same mark regardless of where the binary runs from.
fn app_icon() -> Arc<Image> {
    static ICON: OnceLock<Arc<Image>> = OnceLock::new();
    ICON.get_or_init(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Svg,
            include_bytes!("../../../resources/icon.svg").to_vec(),
        ))
    })
    .clone()
}
