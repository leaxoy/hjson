use super::*;

impl DiffView {
    // ---------- result phase ----------

    pub(super) fn render_result(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        let bg = cx.theme().background;
        self.prepare_side_editors(window, cx);
        let has_diff = matches!(&self.phase, DiffPhase::Result(res) if !res.entries.is_empty());

        // Neither side gets an initial size: with both on the group's flex
        // defaults they take the width evenly, and once measured the even
        // ratio survives a window resize. The lower bound stops a drag from
        // collapsing one side to nothing; the upper stays open so a wide
        // window still gives each half its half.
        let cols = h_resizable("diff-split")
            .child(
                resizable_panel()
                    .size_range(px(240.)..Pixels::MAX)
                    .child(self.render_side_panel(SIDE_A, cx)),
            )
            .child(
                resizable_panel()
                    .size_range(px(240.)..Pixels::MAX)
                    .child(self.render_side_panel(SIDE_B, cx)),
            );

        // No shared title bar: each column header reloads its own side and the
        // status bar carries the summary, the error note, and the navigation.
        let status = self.render_status(window, cx);

        // Differences get their own strip under the columns — the whole change
        // set in one place, instead of hunting for the gutter marks line by line.
        let body: AnyElement = if has_diff {
            v_resizable("diff-body")
                .child(
                    resizable_panel()
                        .size(px(420.))
                        .size_range(px(160.)..px(1400.))
                        .child(cols),
                )
                .child(
                    resizable_panel()
                        .size(px(200.))
                        // Room for the title strip, the column head and a few
                        // rows, even when the user drags it down.
                        .size_range(px(120.)..px(600.))
                        .child(self.render_diff_list(window, cx)),
                )
                .into_any_element()
        } else {
            cols.into_any_element()
        };

        let exemptions = self.render_exemption_controls(cx);
        v_flex()
            .size_full()
            .overflow_hidden()
            .bg(bg)
            .child(exemptions)
            .child(div().flex_1().min_h_0().overflow_hidden().child(body))
            .child(status)
    }

    /// The table of every difference, docked below the two columns. A row click
    /// moves both columns to that entry; the current one is filled.
    fn render_diff_list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity();
        let bg = cx.theme().background;
        let (total, sizes, scroll) = {
            let res = self.result_ref();
            (
                res.entries.len(),
                res.entry_sizes.clone(),
                &res.scroll_entries,
            )
        };
        // Measured once for the whole table so the head and every row agree.
        let path_w = {
            let res = self.result_ref();
            path_col_width(
                &res.entries,
                &crate::views::settings::settings(cx).mono_family(cx),
                window,
            )
        };

        let header = chrome::panel_header(
            tr!("Changes"),
            vec![chrome::stat(
                tr!("%{count} changes", count = total.to_string()),
                cx,
            )],
            vec![chrome::stat(tr!("Click a row to jump to both sides"), cx)],
            cx,
        );

        v_flex()
            .size_full()
            .bg(bg)
            .child(header)
            .child(self.render_diff_head(path_w, cx))
            .child(
                v_flex()
                    .id("diff-list-scroll")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        v_virtual_list(view, "diff-list-rows", sizes, move |this, range, _, cx| {
                            this.render_diff_rows(range, path_w, cx)
                        })
                        .track_scroll(scroll),
                    )
                    .vertical_scrollbar(scroll),
            )
            .into_any_element()
    }

    /// The column titles, laid out with the widths the rows use so the two
    /// stay aligned.
    fn render_diff_head(&self, path_w: Pixels, cx: &gpui_kit::App) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let title = |label: gpui_kit::SharedString| {
            div()
                .text_xs()
                .min_w_0()
                .truncate()
                .text_color(muted)
                .child(label)
        };
        chrome::strip(cx)
            .child(
                div()
                    .w(px(DIFF_COL_KIND_W))
                    .flex_shrink_0()
                    .child(title(tr!("Change"))),
            )
            .child(
                div()
                    .w(path_w)
                    .flex_shrink_1()
                    .min_w_0()
                    .child(title("JSONPath".into())),
            )
            .child(div().flex_1().min_w_0().child(title(tr!("Detail"))))
            .into_any_element()
    }

    /// One row of the diff table: `diff_row` holds the shared column geometry,
    /// this fills it. `path_w` is the table-wide JSONPath width, measured once
    /// by `render_diff_list`.
    fn render_diff_rows(
        &mut self,
        range: Range<usize>,
        path_w: Pixels,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let (list_active, list_hover, muted, danger, success, warning, primary) = {
            let theme = cx.theme();
            (
                theme.list_active,
                theme.list_hover,
                theme.muted_foreground,
                theme.danger,
                theme.success,
                theme.warning,
                theme.primary,
            )
        };
        let mono = crate::views::settings::settings(cx).mono_family(cx);
        let (entries, current) = {
            let res = self.result_ref();
            (&res.entries, res.current)
        };

        range
            .filter_map(|ix| {
                let entry = entries.get(ix)?;
                let color = match entry.kind {
                    DiffKind::Added => success,
                    DiffKind::Removed => danger,
                    DiffKind::Modified => warning,
                    DiffKind::TypeChanged => primary,
                };
                let value = |text: &str, color: Hsla| {
                    div()
                        .min_w_0()
                        .truncate()
                        .font_family(mono.clone())
                        .text_color(if text.is_empty() { muted } else { color })
                        .child(if text.is_empty() {
                            "—".to_string()
                        } else {
                            text.to_string()
                        })
                };
                // Only the values that changed: an added entry has no old side
                // and a removed one no new side.
                let detail = match entry.kind {
                    DiffKind::Added => value(&entry.new, success).into_any_element(),
                    DiffKind::Removed => value(&entry.old, danger).into_any_element(),
                    _ => h_flex()
                        .min_w_0()
                        .items_center()
                        .gap_1()
                        .child(value(&entry.old, danger))
                        .child(Icon::new(IconName::ArrowRight).small().text_color(muted))
                        .child(value(&entry.new, success))
                        .into_any_element(),
                };

                Some(
                    diff_row()
                        .id(SharedString::from(format!("diff-entry-{ix}")))
                        .when(current == Some(ix), |this| this.bg(list_active))
                        .when(current != Some(ix), |this| {
                            this.hover(|this| this.bg(list_hover))
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_entry(ix, cx);
                        }))
                        .child(
                            h_flex()
                                .w(px(DIFF_COL_KIND_W))
                                .flex_shrink_0()
                                .items_center()
                                .gap_1()
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_color(color)
                                        .child(entry.kind.glyph()),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_color(color)
                                        .child(entry.kind.label()),
                                ),
                        )
                        .child(
                            div()
                                .w(path_w)
                                .flex_shrink_1()
                                .min_w_0()
                                .truncate()
                                .font_family(mono.clone())
                                .child(entry.path.clone()),
                        )
                        .child(div().flex_1().min_w_0().child(detail))
                        .into_any_element(),
                )
            })
            .collect()
    }

    /// Initialize once per result, not once per render. Keeping each state
    /// alive preserves its selection, folds and viewport across theme/size changes.
    fn prepare_side_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (wrap, line_numbers, line_h) = {
            let settings = crate::views::settings::settings(cx);
            (
                settings.wrap_raw_lines,
                settings.line_numbers,
                settings.row_height(),
            )
        };
        let colors = {
            let theme = cx.theme();
            [theme.success, theme.danger, theme.warning, theme.primary]
        };
        let color_of = |kind| match kind {
            DiffKind::Added => colors[0],
            DiffKind::Removed => colors[1],
            DiffKind::Modified => colors[2],
            DiffKind::TypeChanged => colors[3],
        };
        let res = match &mut self.phase {
            DiffPhase::Result(res) => res,
            _ => return,
        };
        res.line_h = line_h;
        let refresh_decorations = res.decoration_state != Some((colors, res.current));
        let entry = res.current.and_then(|ix| res.entries.get(ix)).cloned();
        for side in &mut res.sides {
            if side.editor.is_none() {
                let editor = cx.new(|cx| {
                    EditorState::new(window, cx)
                        .default_value(side.pretty.clone())
                        .language("json")
                        .line_number(line_numbers)
                        .soft_wrap(wrap)
                        .scroll_beyond_last_line(Some(0))
                        .scroll_bounce(false)
                });
                let (gutter, active_range) = editor.update(cx, |state, cx| {
                    state.set_readonly(true, cx);
                    (
                        state.create_gutter_decorations_collection(Vec::new(), cx),
                        state.create_range_decorations_collection(Vec::new(), cx),
                    )
                });
                side.editor = Some(editor);
                side.gutter = Some(gutter);
                side.active_range = Some(active_range);
                side.options = Some((wrap, line_numbers));
            }
        }
        // set_cursor_position focuses its editor. Restore whichever control
        // initiated navigation so stepping a diff never steals focus to side B.
        let restore_focus = res.reveal_current.then(|| window.focused(cx)).flatten();
        for (ix, side) in res.sides.iter_mut().enumerate() {
            let editor = side.editor.as_ref().expect("initialized above");
            if side.options != Some((wrap, line_numbers)) {
                editor.update(cx, |state, cx| {
                    state.set_soft_wrap(wrap, window, cx);
                    state.set_line_number(line_numbers, window, cx);
                });
                side.options = Some((wrap, line_numbers));
            }
            let line = entry.as_ref().and_then(|entry| {
                if ix == SIDE_A {
                    entry.a_line
                } else {
                    entry.b_line
                }
            });
            if refresh_decorations {
                let marks = side
                    .marks
                    .iter()
                    .filter_map(|(line, kind)| {
                        side.line_ranges.get(*line).map(|range| {
                            GutterDecoration::new(range.clone()).with_color(color_of(*kind))
                        })
                    })
                    .collect();
                side.gutter
                    .as_ref()
                    .expect("initialized above")
                    .set(marks, cx);
                let active = line
                    .and_then(|line| side.line_ranges.get(line))
                    .map(|range| {
                        RangeDecoration::new(range.clone())
                            .with_style(RangeDecorationStyle::Fill)
                            .with_color(
                                color_of(entry.as_ref().expect("line comes from entry").kind)
                                    .opacity(0.16),
                            )
                    })
                    .into_iter()
                    .collect();
                side.active_range
                    .as_ref()
                    .expect("initialized above")
                    .set(active, cx);
            }
            if res.reveal_current
                && let Some(line) = line
            {
                let position = Position::new(line.min(u32::MAX as usize) as u32, 0);
                editor.update(cx, |state, cx| {
                    state.unfold_at(position, cx);
                    state.set_cursor_position(position, window, cx);
                });
            }
        }
        if let Some(focus) = restore_focus {
            focus.focus(window, cx);
        } else if res.reveal_current {
            window.blur(cx);
        }
        res.decoration_state = Some((colors, res.current));
        res.reveal_current = false;
    }

    fn render_side_panel(&mut self, side: SideIx, cx: &mut Context<Self>) -> AnyElement {
        let (bg, muted) = {
            let theme = cx.theme();
            (theme.background, theme.muted_foreground)
        };
        let mono = crate::views::settings::settings(cx).mono_family(cx);
        let res = self.result_ref();
        let s = &res.sides[side];
        let editor = s
            .editor
            .as_ref()
            .expect("prepared before rendering")
            .clone();
        let title = s.title.clone();
        let font_size = (res.line_h - 8.0).max(1.0);
        let body = Editor::new(&editor)
            .readonly(true)
            .appearance(false)
            .bordered(false)
            .size_full()
            .font_family(mono)
            .text_size(px(font_size))
            .line_height(px(res.line_h))
            .aria_label(if side == SIDE_A {
                tr!("Side A")
            } else {
                tr!("Side B")
            });
        let load = self.side_load_button(side, cx);
        let header = chrome::panel_header(
            if side == SIDE_A { "A" } else { "B" },
            vec![
                div()
                    .text_xs()
                    .text_color(muted)
                    .max_w(px(240.))
                    .min_w_0()
                    .truncate()
                    .child(title)
                    .into_any_element(),
            ],
            vec![load],
            cx,
        );
        v_flex()
            .size_full()
            .bg(bg)
            .child(header)
            .child(
                v_flex()
                    .id(SharedString::from(format!("diff-editor-{side}")))
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .size_full()
                    .py_1()
                    // The editor binds cmd/ctrl-[ and ] to indentation. In a
                    // readonly diff these retain the page's change navigation.
                    .capture_action(cx.listener(|this, _: &Indent, _, cx| this.jump(1, cx)))
                    .capture_action(cx.listener(|this, _: &Outdent, _, cx| this.jump(-1, cx)))
                    .child(body),
            )
            .into_any_element()
    }

    fn render_status(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, danger, success) = (theme.muted_foreground, theme.danger, theme.success);
        let mono = crate::views::settings::settings(cx).mono_family(cx);
        let (entry, counts, total, current) = {
            let res = self.result_ref();
            let counts = if res.entries.is_empty() {
                String::new()
            } else {
                format!(
                    "+{} −{} ~{} ≈{}",
                    res.entries
                        .iter()
                        .filter(|e| e.kind == DiffKind::Added)
                        .count(),
                    res.entries
                        .iter()
                        .filter(|e| e.kind == DiffKind::Removed)
                        .count(),
                    res.entries
                        .iter()
                        .filter(|e| e.kind == DiffKind::Modified)
                        .count(),
                    res.entries
                        .iter()
                        .filter(|e| e.kind == DiffKind::TypeChanged)
                        .count(),
                )
            };
            (
                res.current.and_then(|c| res.entries.get(c)).cloned(),
                counts,
                res.entries.len(),
                res.current,
            )
        };
        let nav = if total == 0 {
            tr!("No changes").to_string()
        } else {
            match current {
                Some(cur) => tr!(
                    "%{current}/%{total} changes",
                    current = (cur + 1).to_string(),
                    total = total.to_string()
                )
                .to_string(),
                None => tr!("%{count} total changes", count = total.to_string()).to_string(),
            }
        };

        let left = match entry.as_ref() {
            Some(entry) => {
                let kind_color: Hsla = match entry.kind {
                    DiffKind::Added => theme.success,
                    DiffKind::Removed => theme.danger,
                    DiffKind::Modified => theme.warning,
                    DiffKind::TypeChanged => theme.primary,
                };
                h_flex()
                    .min_w_0()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .flex_shrink_0()
                            .text_color(kind_color)
                            .child(format!("{} {}", entry.kind.glyph(), entry.kind.label())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family(mono.clone())
                            .min_w_0()
                            .truncate()
                            .child(entry.path.clone()),
                    )
                    .into_any_element()
            }
            None => div()
                .text_xs()
                .text_color(muted)
                .child(if counts.is_empty() && !self.ignored_fields.is_empty() {
                    tr!("No differences after field exemptions")
                } else if counts.is_empty() {
                    tr!("✓ The two documents are identical")
                } else {
                    tr!("Pick a row in the change list below, or step through with ◀ ▶")
                })
                .into_any_element(),
        };

        let right = match entry.as_ref() {
            Some(entry) => h_flex()
                .gap_1()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .font_family(mono.clone())
                        .max_w(px(240.))
                        .min_w_0()
                        .truncate()
                        .text_color(danger)
                        .child(if entry.old.is_empty() {
                            "—".to_string()
                        } else {
                            entry.old.clone()
                        }),
                )
                .child(Icon::new(IconName::ArrowRight).small().text_color(muted))
                .child(
                    div()
                        .text_xs()
                        .font_family(mono.clone())
                        .max_w(px(240.))
                        .min_w_0()
                        .truncate()
                        .text_color(success)
                        .child(if entry.new.is_empty() {
                            "—".to_string()
                        } else {
                            entry.new.clone()
                        }),
                )
                .into_any_element(),
            None => div().into_any_element(),
        };

        let mut status = chrome::status(cx).left(left);

        status = status
            .right(right)
            .right(
                div()
                    .text_xs()
                    .flex_shrink_0()
                    .text_color(muted)
                    .font_family(mono.clone())
                    .child(counts),
            )
            .right(div().text_xs().flex_shrink_0().text_color(muted).child(nav));

        if total > 0 {
            status = status.right(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Button::new("prev-diff")
                            .icon(IconName::ChevronUp)
                            .small()
                            .tooltip(format!(
                                "{}{}",
                                tr!("Previous change"),
                                chrome::shortcut_suffix(&DiffPrev, window)
                            ))
                            .on_click(cx.listener(|this, _, _, cx| this.jump(-1, cx))),
                    )
                    .child(
                        Button::new("next-diff")
                            .icon(IconName::ChevronDown)
                            .small()
                            .tooltip(format!(
                                "{}{}",
                                tr!("Next change"),
                                chrome::shortcut_suffix(&DiffNext, window)
                            ))
                            .on_click(cx.listener(|this, _, _, cx| this.jump(1, cx))),
                    ),
            );
        }

        status = status.right(
            Button::new("swap-sides")
                .icon(IconName::ArrowLeftRight)
                .small()
                .tooltip(tr!("Swap sides (A ⇆ B)"))
                .on_click(cx.listener(|this, _, _, cx| this.swap_sides(cx))),
        );

        status.into_any_element()
    }
}
