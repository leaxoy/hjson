use super::*;

impl DocView {
    pub(super) fn render_raw_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bg = cx.theme().background;
        let (wrap_raw_lines, line_numbers, line_h, mono) = {
            let settings = crate::views::settings::settings(cx);
            (
                settings.wrap_raw_lines,
                settings.line_numbers,
                settings.row_height(),
                settings.mono_family(cx),
            )
        };
        self.raw_editor.update(cx, |editor, cx| {
            editor.set_soft_wrap(wrap_raw_lines, window, cx);
            editor.set_line_number(line_numbers, window, cx);
        });
        let raw_focus = self.raw_editor.focus_handle(cx);
        let raw_text = self.raw_text(cx);
        let line_count = raw_text.lines().count();
        let byte_size = raw_text.len();
        let edit_tooltip = if self.raw_editable {
            tr!("Disable raw editing")
        } else {
            tr!("Enable raw editing")
        };
        let header = chrome::panel_toolbar(
            "doc-raw-toolbar",
            tr!("Raw"),
            vec![
                chrome::stat(tr!("%{count} lines", count = line_count.to_string()), cx),
                crate::app::human_size(byte_size, cx),
            ],
            vec![
                source_menu::header_load_button(
                    "reload-doc",
                    tr!("Reload this document from its source, or switch to another"),
                    (),
                    LoadOptions {
                        url: self.url.clone(),
                        history: source_menu::history_of(&self.app, cx),
                    },
                    cx.weak_entity(),
                ),
                Button::new("toggle-raw-editable")
                    .icon(if self.raw_editable {
                        IconName::LockOpen
                    } else {
                        IconName::Pencil
                    })
                    .ghost()
                    .small()
                    .selected(self.raw_editable)
                    .accessibility_label(edit_tooltip.clone())
                    .tooltip(edit_tooltip)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_raw_editable(window, cx);
                    }))
                    .into_any_element(),
                Button::new("toggle-raw-line-numbers")
                    .icon(IconName::ListOrdered)
                    .ghost()
                    .small()
                    .selected(line_numbers)
                    .accessibility_label(tr!("Show line numbers"))
                    .tooltip(tr!("Show line numbers"))
                    .on_click(cx.listener(|_, _, _, cx| {
                        crate::views::settings::update_settings(cx, |settings| {
                            settings.line_numbers = !settings.line_numbers;
                        });
                    }))
                    .into_any_element(),
                Button::new("expand-raw-folds")
                    .icon(IconName::ChevronsUpDown)
                    .ghost()
                    .small()
                    .tooltip(format!(
                        "{}{}",
                        tr!("Expand all"),
                        chrome::shortcut_suffix(&ExpandAll, window)
                    ))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.set_raw_depth(true, window, cx);
                    }))
                    .into_any_element(),
                Button::new("collapse-raw-folds")
                    .icon(IconName::ChevronsDownUp)
                    .ghost()
                    .small()
                    .tooltip(format!(
                        "{}{}",
                        tr!("Collapse all"),
                        chrome::shortcut_suffix(&CollapseAll, window)
                    ))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.set_raw_depth(false, window, cx);
                    }))
                    .into_any_element(),
                Button::new("compare-doc")
                    .icon(gpui_kit::assets::IconName::GitCompare)
                    .ghost()
                    .small()
                    .tooltip(tr!(
                        "Put this document on side A and compare it with another"
                    ))
                    .on_click(cx.listener(|this, _, _window, cx| {
                        let document = match this.diff_snapshot(cx) {
                            Ok(document) => document,
                            Err(err) => {
                                this.say(false, err.message(), cx);
                                return;
                            }
                        };
                        this.app
                            .update(cx, |app, cx| app.new_diff_tab(Some((0, document)), cx))
                            .ok();
                        cx.notify();
                    }))
                    .into_any_element(),
                Button::new("copy-raw")
                    .icon(IconName::Copy)
                    .ghost()
                    .small()
                    .tooltip(tr!("Copy all JSON"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let text = this.raw_text(cx);
                        this.copy_text(text, tr!("All JSON copied").to_string(), cx);
                    }))
                    .into_any_element(),
                Button::new("save-json")
                    .icon(IconName::Download)
                    .ghost()
                    .small()
                    .tooltip(tr!("Save JSON"))
                    .dropdown_menu({
                        let doc_view = cx.weak_entity();
                        move |menu, _, _| {
                            let pretty_doc_view = doc_view.clone();
                            let compact_doc_view = doc_view.clone();
                            menu.item(
                                PopupMenuItem::new(tr!("Save formatted JSON"))
                                    .icon(IconName::FileBraces)
                                    .on_click(move |_, _, cx| {
                                        let _ = pretty_doc_view.update(cx, |doc_view, cx| {
                                            doc_view.save_json(false, cx)
                                        });
                                    }),
                            )
                            .item(
                                PopupMenuItem::new(tr!("Save compact JSON"))
                                    .icon(IconName::Braces)
                                    .on_click(move |_, _, cx| {
                                        let _ = compact_doc_view.update(cx, |doc_view, cx| {
                                            doc_view.save_json(true, cx)
                                        });
                                    }),
                            )
                        }
                    })
                    .into_any_element(),
            ],
            cx,
        );
        let font_size = (line_h - 8.0).max(1.0);
        let body = Editor::new(&self.raw_editor)
            .readonly(!self.raw_editable)
            .appearance(false)
            .bordered(false)
            .size_full()
            .font_family(mono)
            .text_size(px(font_size))
            .line_height(px(line_h))
            .aria_label(tr!("Raw JSON"));
        v_flex()
            .size_full()
            .bg(bg)
            .child(header)
            .child(
                v_flex()
                    .id("raw-scroll")
                    .relative()
                    .flex_1()
                    .size_full()
                    .py_1()
                    .key_context(RAW_CONTEXT)
                    .track_focus(&raw_focus)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.focus_raw_editor(window, cx);
                    }))
                    .child(body),
            )
            .when(self.doc.pretty.is_empty(), |this| {
                this.child(chrome::empty_state(
                    IconName::FileText,
                    tr!("Empty document"),
                    None,
                ))
            })
            .into_any_element()
    }
}
