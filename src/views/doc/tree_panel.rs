use super::*;

impl DocView {
    pub(super) fn render_tree_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bg = cx.theme().background;
        let doc = self.doc.clone();
        let doc_view = cx.weak_entity();
        self.tree_list.remeasure();
        let query_active = self.query.is_some();
        let match_count = self.match_count;
        let search_placeholder = tr!("Search keys or values… (filters the tree)");
        self.search.update(cx, |search, cx| {
            search.set_placeholder(search_placeholder, window, cx)
        });

        let header = chrome::panel_toolbar(
            "doc-tree-toolbar",
            tr!("Tree"),
            vec![if query_active {
                chrome::stat(
                    tr!(
                        "Filtered · %{count} matches",
                        count = match_count.to_string()
                    ),
                    cx,
                )
            } else {
                chrome::stat(
                    tr!("%{count} nodes", count = doc.nodes.len().to_string()),
                    cx,
                )
            }],
            vec![
                div()
                    .w(px(220.))
                    .min_w(px(120.))
                    .child(
                        Input::new(&self.search)
                            .small()
                            .cleanable(true)
                            .focus_bordered(false),
                    )
                    .into_any_element(),
                Button::new("expand-all")
                    .icon(IconName::ChevronsUpDown)
                    .ghost()
                    .small()
                    .tooltip(format!(
                        "{}{}",
                        tr!("Expand all"),
                        chrome::shortcut_suffix(&ExpandAll, window)
                    ))
                    .on_click(cx.listener(|this, _, _, cx| this.expand_tree_all(cx)))
                    .into_any_element(),
                Button::new("collapse-all")
                    .icon(IconName::ChevronsDownUp)
                    .ghost()
                    .small()
                    .tooltip(format!(
                        "{}{}",
                        tr!("Collapse all"),
                        chrome::shortcut_suffix(&CollapseAll, window)
                    ))
                    .on_click(cx.listener(|this, _, _, cx| this.collapse_tree_all(cx)))
                    .into_any_element(),
            ],
            cx,
        );

        let tree_view = JsonTreeView::new(JsonTreeViewProps {
            doc: doc.clone(),
            rows: self.tree_rows.clone(),
            selected_node: self.selected_node,
            list_state: self.tree_list.clone(),
            focus_handle: self.tree_focus.clone(),
            empty_message: tr!("No matching nodes"),
            on_activate: Some(Rc::new({
                let doc_view = doc_view.clone();
                move |node_id, window, cx| {
                    let _ = doc_view.update(cx, |doc_view, cx| {
                        doc_view.activate_tree_node(node_id, window, cx);
                    });
                }
            })),
            on_context_menu: Some(Rc::new({
                let doc = doc.clone();
                let doc_view = doc_view.clone();
                move |node_id, menu, window, cx| {
                    node_context_menu(node_id, &doc, &doc_view, menu, window, cx)
                }
            })),
            on_select_up: Some(Rc::new({
                let doc_view = doc_view.clone();
                move |window, cx| {
                    let _ = doc_view.update(cx, |doc_view, cx| {
                        doc_view.move_tree_selection(-1, window, cx);
                    });
                }
            })),
            on_select_down: Some(Rc::new({
                let doc_view = doc_view.clone();
                move |window, cx| {
                    let _ = doc_view.update(cx, |doc_view, cx| {
                        doc_view.move_tree_selection(1, window, cx);
                    });
                }
            })),
            on_select_left: Some(Rc::new({
                let doc_view = doc_view.clone();
                move |window, cx| {
                    let _ = doc_view.update(cx, |doc_view, cx| {
                        doc_view.collapse_tree_selection(window, cx);
                    });
                }
            })),
            on_select_right: Some(Rc::new({
                let doc_view = doc_view.clone();
                move |_, cx| {
                    let _ = doc_view.update(cx, |doc_view, cx| {
                        doc_view.expand_tree_selection(cx);
                    });
                }
            })),
            on_confirm: Some(Rc::new({
                let doc_view = doc_view.clone();
                move |_, cx| {
                    let _ = doc_view.update(cx, |doc_view, cx| {
                        doc_view.confirm_tree_selection(cx);
                    });
                }
            })),
        })
        .render(window, cx);

        v_flex()
            .size_full()
            .bg(bg)
            .child(header)
            .child(div().flex_1().min_h_0().relative().p_1().child(tree_view))
            .into_any_element()
    }
}

/// The right-click menu for a node: copies with paths.
fn node_context_menu(
    id: usize,
    doc: &JsonTree,
    doc_view: &WeakEntity<DocView>,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let node = &doc.nodes[id];
    let json_path = doc.json_path(id);

    let open_node = {
        let doc_view = doc_view.clone();
        PopupMenuItem::new(tr!("Open node in new tab"))
            .icon(IconName::ExternalLink)
            .on_click(move |_, window, cx| {
                let _ = doc_view.update(cx, |doc_view, cx| {
                    doc_view.open_node_in_new_tab(id, window, cx);
                });
            })
    };
    let parse_string = {
        let doc_view = doc_view.clone();
        let text = string_value_text(doc, id).unwrap_or_default();
        let can_parse = is_json_text(&text);
        PopupMenuItem::new(tr!("Parse string and open in new tab"))
            .icon(IconName::FileBraces)
            .disabled(!can_parse)
            .on_click(move |_, window, cx| {
                let _ = doc_view.update(cx, |doc_view, cx| {
                    doc_view.open_json_string_node_in_new_tab(id, window, cx);
                });
            })
    };
    let copy_key = {
        let doc_view = doc_view.clone();
        let text = node.key.clone().unwrap_or_default();
        PopupMenuItem::new(tr!("Copy key"))
            .icon(IconName::Quote)
            .disabled(node.key.is_none())
            .on_click(move |_, _, cx| {
                copy_via_doc_view(&doc_view, text.clone(), tr!("Key copied").to_string(), cx);
            })
    };
    let copy_value = copy_item(
        tr!("Copy value"),
        IconName::Copy,
        value_to_copy(doc, id),
        tr!("Value copied"),
        doc_view,
    );
    let copy_string = {
        let doc_view = doc_view.clone();
        let text = scalar_value_text(doc, id);
        PopupMenuItem::new(tr!("Copy string without quotes"))
            .icon(IconName::Quote)
            .disabled(node.typ != JsonType::String)
            .on_click(move |_, _, cx| {
                copy_via_doc_view(
                    &doc_view,
                    text.clone(),
                    tr!("String copied").to_string(),
                    cx,
                );
            })
    };
    let copy_pretty = copy_item(
        tr!("Copy subtree (formatted)"),
        IconName::FileBraces,
        doc.subtree_json(id).unwrap_or_default(),
        tr!("Formatted subtree copied"),
        doc_view,
    );
    let copy_compact = copy_item(
        tr!("Copy subtree (compact)"),
        IconName::Braces,
        doc.subtree_json_compact(id).unwrap_or_default(),
        tr!("Compact subtree copied"),
        doc_view,
    );
    let copy_all = copy_item(
        tr!("Copy all JSON"),
        IconName::Copy,
        doc.pretty.clone(),
        tr!("Everything copied"),
        doc_view,
    );

    let paths_menu = {
        let doc_view = doc_view.clone();
        let entries = [
            ("JSONPath", doc.json_path(id), IconName::Hash),
            ("JSON Pointer", doc.json_pointer(id), IconName::GitBranch),
            ("jq", doc.jq_path(id), IconName::Terminal),
        ];
        PopupMenu::build(window, cx, |sub, _, _| {
            let mut menu = sub;
            for (label, path, icon) in entries {
                let doc_view = doc_view.clone();
                let toast = tr!("%{label} path copied", label = label).to_string();
                menu = menu.item(
                    PopupMenuItem::new(format!("{label} — {path}"))
                        .icon(icon)
                        .on_click(move |_, _, cx| {
                            copy_via_doc_view(&doc_view, path.clone(), toast.clone(), cx);
                        }),
                );
            }
            menu
        })
    };

    menu.max_w(px(560.))
        .label(format!("{} · {}", json_path, node.typ.label()))
        .separator()
        .item(open_node)
        .item(parse_string)
        .separator()
        .item(copy_key)
        .item(copy_value)
        .item(copy_string)
        .item(copy_pretty)
        .item(copy_compact)
        .separator()
        .item(PopupMenuItem::submenu(tr!("Copy path…"), paths_menu))
        .separator()
        .item(copy_all)
}
