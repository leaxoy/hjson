use super::*;

impl DocView {
    pub(super) fn render_status(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let mono = crate::views::settings::settings(cx).mono_family(cx);
        let syntax_mode = crate::syntax::SyntaxMode::current(cx);

        let selected = self.selected_node.map(|id| {
            let typ = self.doc.nodes[id].typ;
            (typ, self.doc.json_path(id))
        });

        let left = match selected {
            Some((typ, path)) => h_flex()
                .min_w_0()
                .gap_2()
                .items_center()
                .child(
                    Icon::new(type_icon(typ))
                        .small()
                        .text_color(type_color(typ, syntax_mode)),
                )
                .child(
                    div()
                        .text_xs()
                        .flex_shrink_0()
                        .text_color(type_color(typ, syntax_mode))
                        .child(typ.label()),
                )
                .child(div().text_xs().font_family(mono).child(path))
                .into_any_element(),
            None => div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(tr!(
                    "Select a node to see its JSONPath and locate it in the raw text — ↑↓ nodes, Tab panes, ⌘F filter"
                ))
                .into_any_element(),
        };

        let right = if let Some(toast) = self.toast.clone() {
            let ok = toast.is_success();
            let text = toast.message();
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    Icon::new(if ok {
                        IconName::CircleCheck
                    } else {
                        IconName::CircleAlert
                    })
                    .small()
                    .text_color(if ok { theme.success } else { theme.danger }),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(text),
                )
                .into_any_element()
        } else {
            div().into_any_element()
        };

        chrome::status(cx)
            .left(left)
            .right(right)
            .right(self.pane_mode_toggle(cx))
            .into_any_element()
    }
}
