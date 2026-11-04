use super::HJsonApp;
use crate::components::chrome;
use crate::components::source_menu::{LoadMethod, LoadTarget, open_manual_dialog, open_url_dialog};
use crate::document::{self, LoadedDocument, SourceKind};
use crate::error::{AppError, Status};
use crate::tr;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    WeakEntity, Window, div, px,
};

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

    pub(super) fn set_status(&mut self, ok: bool, text: impl Into<String>) {
        self.status = Some(Status::from_parts(ok, text));
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
                self.set_status(false, AppError::parse(err.as_str()).message());
                Err(err)
            }
        }
    }

    pub fn open_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = cx.read_from_clipboard().and_then(|item| item.text());
        match text {
            Some(text) if !text.trim().is_empty() => match document::from_clipboard(text) {
                Ok(document) => {
                    let _ = self.submit(document, window, cx);
                }
                Err(err) => self.set_status(false, err.message()),
            },
            Some(_) => self.set_status(false, tr!("The clipboard is empty")),
            None => self.set_status(false, tr!("No text on the clipboard")),
        }
        cx.notify();
    }

    pub fn open_manual(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        match document::from_manual(text.to_string()) {
            Ok(document) => {
                let _ = self.submit(document, window, cx);
            }
            Err(err) => self.set_status(false, err.message()),
        }
        cx.notify();
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
        match document::from_sample() {
            Ok(document) => {
                let _ = self.submit(document, window, cx);
            }
            Err(err) => self.set_status(false, err.message()),
        }
        cx.notify();
    }

    pub fn open_doc(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self.app.update(cx, |app, cx| app.open_doc(id, window, cx));
    }

    pub fn fetch_url(
        &mut self,
        url: String,
        custom_title: Option<String>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let url = url.trim().to_string();
        if url.is_empty() {
            self.set_status(false, tr!("Enter a URL").to_string());
            cx.notify();
            return;
        }
        self.status = Some(Status::success(
            tr!("Requesting %{url} …", url = url.clone()).to_string(),
        ));
        cx.notify();
        let task =
            cx.background_spawn(async move { document::from_url(url.clone(), custom_title) });

        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(document) => {
                    let url = document.url.clone().unwrap_or_default();
                    match this.submit(document, window, cx) {
                        Ok(_) => this
                            .set_status(true, tr!("Loaded %{url}", url = url.clone()).to_string()),
                        Err(err) => {
                            this.set_status(false, AppError::parse(err).message());
                        }
                    }
                    cx.notify();
                }
                Err(e) => {
                    this.set_status(false, e.message());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn pick_file(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let outcome = match receiver.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => None,
                Ok(Err(e)) => {
                    this.update(cx, |this, cx| {
                        this.set_status(false, AppError::open_file(e).message());
                        cx.notify();
                    })
                    .ok();
                    return;
                }
                Err(_) => None,
            };
            let Some(path) = outcome else { return };
            let read = cx.background_spawn(async move { document::from_file(path) });
            let read_result = read.await;
            this.update_in(cx, |this, window, cx| {
                match read_result {
                    Ok(document) => match this.submit(document, window, cx) {
                        Ok(_) => this.set_status(true, tr!("File opened")),
                        Err(err) => this.set_status(false, AppError::parse(err).message()),
                    },
                    Err(e) => this.set_status(false, e.message()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl LoadTarget for HomeView {
    type Slot = ();

    /// The home page has one target — itself — so the shared load menu and the
    /// import list both land here.
    fn apply_load(
        &mut self,
        _slot: (),
        method: LoadMethod,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match method {
            LoadMethod::Clipboard => self.open_clipboard(window, cx),
            LoadMethod::PickFile => self.pick_file(window, cx),
            LoadMethod::FetchUrl { url, title } => self.fetch_url(url, title, window, cx),
            LoadMethod::Manual(text) => self.open_manual(&text, window, cx),
            LoadMethod::Sample => self.open_sample(window, cx),
            LoadMethod::History(id) => self.open_doc(id, window, cx),
            // Nothing to re-fetch: a home tab has no document of its own.
            LoadMethod::RefetchUrl => {}
        }
    }
}

impl Render for HomeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = window;
        self.render_home(cx)
    }
}

impl HomeView {
    /// OK-JSON-style home: a compact brand bar and the centered list of import
    /// actions. History is a sidebar of its own now (`⌘⇧H`), so this page stays
    /// about one thing — getting a document in.
    fn render_home(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        // ---- brand bar ----
        let header = h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(
                Icon::new(gpui_kit::assets::IconName::Braces)
                    .with_size(gpui_kit::component::Size::Medium)
                    .text_color(theme.primary),
            )
            .child(
                div()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("HJson"),
            )
            .child(h_flex().flex_1())
            .child(chrome::stat(
                tr!("Every document stays on this machine"),
                cx,
            ));

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
                "⌘⇧V",
                cx.listener(|this, _, window, cx| this.open_clipboard(window, cx)),
                cx,
            ))
            .child(chrome::divider(cx))
            .child(action_row(
                "open-url",
                SourceKind::Url,
                tr!("Fetch from URL"),
                tr!("enter the address in a dialog"),
                cx.listener(|this, _, window, cx| this.open_url_dialog(window, cx)),
                cx,
            ))
            .child(chrome::divider(cx))
            .child(action_row(
                "open-file",
                SourceKind::File,
                tr!("Open file"),
                "⌘O",
                cx.listener(|this, _, window, cx| this.pick_file(window, cx)),
                cx,
            ))
            .child(chrome::divider(cx))
            .child(action_row(
                "manual-input",
                SourceKind::Manual,
                tr!("Manual input"),
                tr!("paste the JSON in a dialog"),
                cx.listener(|this, _, window, cx| this.open_manual_dialog(window, cx)),
                cx,
            ))
            .child(chrome::divider(cx))
            .child(action_row(
                "open-sample",
                SourceKind::Sample,
                tr!("Sample document"),
                tr!("built-in data"),
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
    hint: impl Into<SharedString>,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &Context<HomeView>,
) -> AnyElement {
    let label = label.into();
    let hint = hint.into();
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
                .child(chrome::stat(hint, cx)),
        )
        .into_any_element()
}
