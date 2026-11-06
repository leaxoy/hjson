use super::*;

impl DiffView {
    // ---------- result phase ----------

    pub(super) fn render_result(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        let bg = cx.theme().background;
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

        v_flex()
            .size_full()
            .overflow_hidden()
            .bg(bg)
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

    fn render_side_panel(&mut self, side: SideIx, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity();
        let (bg, muted) = {
            let theme = cx.theme();
            (theme.background, theme.muted_foreground)
        };
        let wanted_h = crate::views::settings::settings(cx).row_height();
        if let DiffPhase::Result(res) = &mut self.phase
            && res.line_h != wanted_h
        {
            res.line_h = wanted_h;
            let line_h = res.line_h;
            for side_lines in res.sides.iter_mut() {
                side_lines.line_sizes =
                    Rc::new(vec![size(px(640.), px(line_h)); side_lines.lines.len()]);
            }
        }
        let (sizes, scroll, lines_empty, title) = {
            let res = self.result_ref();
            let s = &res.sides[side];
            let scroll = if side == SIDE_A {
                &res.scroll_a
            } else {
                &res.scroll_b
            };
            (
                s.line_sizes.clone(),
                scroll,
                s.lines.is_empty(),
                s.title.clone(),
            )
        };
        // This side's reload entry sits above its own column, next to the
        // source title it belongs to.
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
                    .id(SharedString::from(format!("diff-scroll-{side}")))
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .py_1()
                    .child(
                        v_virtual_list(
                            view,
                            SharedString::from(format!("diff-rows-{side}")),
                            sizes,
                            move |this, range, window, cx| {
                                this.render_side_rows(side, range, window, cx)
                            },
                        )
                        .track_scroll(scroll),
                    )
                    .vertical_scrollbar(scroll),
            )
            .when(lines_empty, |this| {
                this.child(chrome::empty_state(
                    IconName::FileText,
                    tr!("Empty document"),
                    None,
                ))
            })
            .into_any_element()
    }

    fn render_side_rows(
        &mut self,
        side: SideIx,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        // Batch together everything the row builder needs before the loop: the
        // theme borrows cx and the loop wants plain data.
        let row_h = {
            let res = self.result_ref();
            res.line_h
        };
        let settings = crate::views::settings::settings(cx);
        // Line numbers are a document-wide setting, so the diff obeys it too.
        let show_gutter = settings.line_numbers;
        let (border_default, success, danger, warning, primary) = {
            let theme = cx.theme();
            (
                theme.border,
                theme.success,
                theme.danger,
                theme.warning,
                theme.primary,
            )
        };
        let color_of = |kind: DiffKind| -> Hsla {
            match kind {
                DiffKind::Added => success,
                DiffKind::Removed => danger,
                DiffKind::Modified => warning,
                DiffKind::TypeChanged => primary,
            }
        };

        let (lines, marks, entry_of_line, current) = {
            let res = self.result_ref();
            let s = &res.sides[side];
            (s.lines.clone(), &s.marks, &s.entry_of_line, res.current)
        };

        range
            .map(|ix| {
                let mark = marks.get(ix).copied().flatten();
                let entry_ix = entry_of_line.get(&ix).copied();
                let is_current = entry_ix.is_some() && entry_ix == current;

                let marker = mark.map(|kind| JsonTextMarker {
                    glyph: kind.glyph().to_string().into(),
                    color: color_of(kind),
                });
                let on_click: Option<JsonTextClickHandler> = entry_ix.map(|entry| {
                    Box::new(cx.listener(move |this, _: &gpui_kit::ClickEvent, _, cx| {
                        this.select_entry(entry, cx);
                    })) as JsonTextClickHandler
                });
                let row = JsonTextView::new(JsonTextViewProps {
                    id: Some(SharedString::from(format!("diff-line-{side}-{ix}"))),
                    ix,
                    lines: lines.clone(),
                    line_h: row_h,
                    mode: JsonTextMode::Clipped,
                    gutter: if show_gutter {
                        JsonTextGutter::Fixed(px(56.))
                    } else {
                        JsonTextGutter::Hidden
                    },
                    marker_column: true,
                    active: is_current,
                    marker,
                    border_color: Some(mark.map(color_of).unwrap_or(border_default)),
                    on_click,
                })
                .render(window, cx);
                row.into_any_element()
            })
            .collect()
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
                .child(if counts.is_empty() {
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
