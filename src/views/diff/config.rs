use super::*;

impl DiffView {
    // ---------- config phase ----------

    /// What one side's load menu can offer: its own URL (for re-fetch) plus
    /// the session history.
    pub(super) fn side_load_options(
        &self,
        side: SideIx,
        cx: &gpui_kit::App,
    ) -> crate::components::source_menu::LoadOptions {
        let url = match &self.phase {
            DiffPhase::Config(cfg) => cfg.sides[side].url.clone(),
            DiffPhase::Result(res) => res.sources[side].url.clone(),
        };
        crate::components::source_menu::LoadOptions {
            url,
            history: crate::components::source_menu::history_of(&self.app, cx),
        }
    }

    /// The side's one load control, docked in that side's titlebar — the same
    /// shape as a document's own header: an icon button whose menu holds every
    /// source. The shared load button owns the glyph, so all JSON-loading
    /// surfaces look the same.
    pub(super) fn side_load_button(&self, side: SideIx, cx: &mut Context<Self>) -> AnyElement {
        let (kind, title, filled) = match &self.phase {
            DiffPhase::Config(cfg) => (
                cfg.sides[side]
                    .kind
                    .map(|k| k.label())
                    .unwrap_or(tr!("Nothing yet")),
                cfg.sides[side].title.clone().unwrap_or_default(),
                cfg.sides[side].title.is_some(),
            ),
            DiffPhase::Result(res) => (
                res.sources[side].kind.label(),
                res.sources[side].title.clone(),
                true,
            ),
        };
        let side_label = if side == SIDE_A {
            tr!("Side A")
        } else {
            tr!("Side B")
        };
        let verb = if filled { tr!("Reload") } else { tr!("Load") };
        crate::components::source_menu::header_load_button(
            SharedString::from(format!("load-{side}")),
            tr!(
                "%{verb} %{side} (current: %{title} · %{kind})",
                verb = verb.to_string(),
                side = side_label.to_string(),
                title = title.to_string(),
                kind = kind.to_string()
            )
            .to_string(),
            side,
            self.side_load_options(side, cx),
            cx.weak_entity(),
        )
    }

    pub(super) fn render_config(&mut self, cx: &mut Context<Self>) -> impl gpui_kit::IntoElement {
        // Build all child elements first — they borrow cx mutably — then take
        // the (borrowing) theme snapshot for styling the container itself.
        let ready = self.config_ready();
        let notes = self.render_notes(cx);
        let card_a = self.render_side_card(SIDE_A, cx);
        let card_b = self.render_side_card(SIDE_B, cx);
        let theme = cx.theme();
        let muted = theme.muted_foreground;

        let page = v_flex()
            .id("diff-config")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(24.))
            .py(px(20.))
            .gap_4()
            .child(div().text_sm().text_color(muted).child(
                tr!("One JSON per side, matched by JSONPath · object keys are order-independent · array items align by index"),
            ))
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .items_stretch()
                    .child(card_a)
                    .child(card_b),
            )
            .children(notes)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_center()
                    .child(if ready {
                        Button::new("start-diff")
                            .label(tr!("Start comparison"))
                            .primary()
                            .on_click(cx.listener(|this, _, _, cx| this.run_diff(cx)))
                    } else {
                        Button::new("start-diff")
                            .label(tr!("Put content on both sides to start comparing"))
                            .disabled(true)
                    }),
            );

        v_flex()
            .size_full()
            .overflow_hidden()
            .bg(theme.background)
            .child(page)
    }

    /// One side slot: its titlebar carries the side's name and its single load
    /// control — the same shape as a document's own header — and the body shows
    /// either the empty drop area or what the side holds.
    fn render_side_card(&mut self, side: SideIx, cx: &mut Context<Self>) -> AnyElement {
        let (border, muted, primary) = {
            let theme = cx.theme();
            (theme.border, theme.muted_foreground, theme.primary)
        };
        let side_label = if side == SIDE_A {
            tr!("Side A")
        } else {
            tr!("Side B")
        };
        let (title, kind, busy, preview) = match &self.phase {
            DiffPhase::Config(cfg) => {
                let s = &cfg.sides[side];
                let preview: String = s
                    .text
                    .as_deref()
                    .unwrap_or("")
                    .replace(['\n', '\r', '\t'], " ")
                    .chars()
                    .take(64)
                    .collect();
                (s.title.clone(), s.kind, s.busy, preview)
            }
            _ => unreachable!(),
        };
        let load = self.side_load_button(side, cx);
        // A fetch in flight needs a surface of its own: the load menu closes as
        // soon as it fires, so the side's titlebar says what is happening.
        let busy_chip = busy.then(|| {
            Button::new(SharedString::from(format!("side-busy-{side}")))
                .label(tr!("Requesting…"))
                .small()
                .disabled(true)
                .into_any_element()
        });

        let card = v_flex()
            .id(SharedString::from(format!("side-card-{side}")))
            .w_full()
            .flex_1()
            .rounded_lg()
            .border_1()
            .border_color(border)
            .p_3()
            .gap_3()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(muted)
                            .child(side_label),
                    )
                    .child(div().flex_1())
                    .children(busy_chip)
                    .child(load),
            );

        if title.is_none() {
            return card
                .child(chrome::empty_state(
                    IconName::Download,
                    tr!("Nothing yet"),
                    Some(tr!(
                        "Use Load in the top-right: paste / open file / fetch URL / pick from history"
                    )),
                ))
                .into_any_element();
        }

        let mono = crate::views::settings::settings(cx).mono_family(cx);
        card.child(
            h_flex()
                .gap_2()
                .items_center()
                .child(Icon::new(IconName::Braces).small().text_color(primary))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .min_w_0()
                        .truncate()
                        .child(title.unwrap_or_default()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(kind.map(|k| k.label()).unwrap_or("—".into())),
                )
                .child(div().flex_1())
                .child(
                    Button::new(SharedString::from(format!("side-clear-{side}")))
                        .icon(IconName::X)
                        .ghost()
                        .small()
                        .tooltip(tr!("Remove this side"))
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.with_config(|cfg| cfg.sides[side].clear());
                            cx.notify();
                        })),
                ),
        )
        .child(
            div()
                .text_xs()
                .font_family(mono)
                .text_color(muted)
                .truncate()
                .child(preview),
        )
        .into_any_element()
    }

    fn render_notes(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (notes_a, notes_b) = match &self.phase {
            DiffPhase::Config(cfg) => (cfg.sides[0].note.clone(), cfg.sides[1].note.clone()),
            _ => (None, None),
        };
        if notes_a.is_none() && notes_b.is_none() {
            return None;
        }
        let (danger, success) = {
            let theme = cx.theme();
            (theme.danger, theme.success)
        };
        let mk = |n: Option<Status>| {
            n.map(|s| {
                let ok = s.is_success();
                let text = s.message();
                div()
                    .text_xs()
                    .text_color(if ok { success } else { danger })
                    .child(text)
                    .into_any_element()
            })
        };
        let (a, b) = (mk(notes_a), mk(notes_b));
        let joiner = |n: Option<AnyElement>| match n {
            Some(el) => el,
            None => div().into_any_element(),
        };
        Some(
            h_flex()
                .w_full()
                .gap_4()
                .child(
                    div()
                        .w_full()
                        .flex_1()
                        .flex()
                        .justify_center()
                        .child(joiner(a)),
                )
                .child(
                    div()
                        .w_full()
                        .flex_1()
                        .flex()
                        .justify_center()
                        .child(joiner(b)),
                )
                .into_any_element(),
        )
    }
}
