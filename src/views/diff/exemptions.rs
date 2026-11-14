//! Searchable, draft-only field selection for one comparison's exemptions.

use std::{collections::BTreeSet, rc::Rc};

use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, VirtualListScrollHandle,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement as _,
    v_flex, v_virtual_list,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Render, ScrollStrategy, SharedString, Size,
    Styled as _, Subscription, Window, div, px, size,
};
use serde_json::Value;

use crate::tr;

/// Literal object keys from both documents, including properties under arrays
/// and exempt parents. Never collect array indices or string values.
pub(super) fn collect_field_names(value: &Value, fields: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (name, value) in map {
                fields.insert(name.clone());
                collect_field_names(value, fields);
            }
        }
        Value::Array(items) => {
            for value in items {
                collect_field_names(value, fields);
            }
        }
        _ => {}
    }
}

struct FieldSelection {
    fields: Vec<String>,
    search_names: Vec<String>,
    visible: Vec<usize>,
    selected: BTreeSet<String>,
}

impl FieldSelection {
    fn new(mut fields: BTreeSet<String>, selected: BTreeSet<String>) -> Self {
        fields.extend(selected.iter().cloned());
        let fields: Vec<_> = fields.into_iter().collect();
        Self {
            search_names: fields.iter().map(|name| name.to_lowercase()).collect(),
            visible: (0..fields.len()).collect(),
            fields,
            selected,
        }
    }

    fn filter(&mut self, query: &str) {
        let query = query.trim().to_lowercase();
        self.visible = self
            .search_names
            .iter()
            .enumerate()
            .filter_map(|(ix, name)| name.contains(&query).then_some(ix))
            .collect();
    }

    fn set_checked(&mut self, ix: usize, checked: bool) {
        if let Some(name) = self.fields.get(ix) {
            if checked {
                self.selected.insert(name.clone());
            } else {
                self.selected.remove(name);
            }
        }
    }

    fn set_visible_checked(&mut self, checked: bool) {
        for &ix in &self.visible {
            if checked {
                self.selected.insert(self.fields[ix].clone());
            } else {
                self.selected.remove(&self.fields[ix]);
            }
        }
    }
}

pub(super) struct FieldPicker {
    selection: FieldSelection,
    search: Entity<InputState>,
    scroll: VirtualListScrollHandle,
    sizes: Rc<Vec<Size<Pixels>>>,
    _search_sub: Subscription,
}

const FIELD_ROW_H: f32 = 32.0;

impl FieldPicker {
    pub(super) fn new(
        fields: BTreeSet<String>,
        selected: BTreeSet<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let selection = FieldSelection::new(fields, selected);
        let sizes = Rc::new(vec![
            size(px(400.), px(FIELD_ROW_H));
            selection.visible.len()
        ]);
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr!("Search field names…")));
        let search_sub = cx.subscribe_in(&search, window, |this, state, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.selection.filter(&state.read(cx).value().to_string());
                this.sizes = Rc::new(vec![
                    size(px(400.), px(FIELD_ROW_H));
                    this.selection.visible.len()
                ]);
                if !this.selection.visible.is_empty() {
                    this.scroll.scroll_to_item(0, ScrollStrategy::Top);
                }
                cx.notify();
            }
        });
        Self {
            selection,
            search,
            sizes,
            scroll: VirtualListScrollHandle::new(),
            _search_sub: search_sub,
        }
    }

    pub(super) fn search_focus(&self, cx: &App) -> FocusHandle {
        self.search.focus_handle(cx)
    }

    pub(super) fn selected(&self) -> &BTreeSet<String> {
        &self.selection.selected
    }
}

impl Render for FieldPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let view = cx.entity();
        let empty = self.selection.fields.is_empty();
        let no_matches = self.selection.visible.is_empty();
        let counts = tr!(
            "%{selected} selected · %{visible}/%{total} fields",
            selected = self.selection.selected.len().to_string(),
            visible = self.selection.visible.len().to_string(),
            total = self.selection.fields.len().to_string()
        );
        let list = v_virtual_list(
            view,
            "exemption-field-rows",
            self.sizes.clone(),
            |this, range, _, cx| {
                range
                    .filter_map(|row| {
                        let ix = *this.selection.visible.get(row)?;
                        let name = this.selection.fields[ix].clone();
                        let label: SharedString = if name.is_empty() {
                            tr!("(empty field name)")
                        } else {
                            name.clone().into()
                        };
                        Some(
                            div()
                                .h(px(FIELD_ROW_H))
                                .w_full()
                                .flex()
                                .items_center()
                                .px_2()
                                .child(
                                    Checkbox::new(SharedString::from(format!("exempt-field-{ix}")))
                                        .checked(this.selection.selected.contains(&name))
                                        .small()
                                        .min_w_0()
                                        .w_full()
                                        .accessibility_label(label.clone())
                                        .tooltip(label.clone())
                                        .child(div().min_w_0().truncate().child(label))
                                        .on_change(cx.listener(
                                            move |this, checked: &bool, _, cx| {
                                                this.selection.set_checked(ix, *checked);
                                                cx.notify();
                                            },
                                        )),
                                )
                                .into_any_element(),
                        )
                    })
                    .collect()
            },
        )
        .track_scroll(&self.scroll);
        v_flex()
            .w_full()
            .gap_2()
            .child(Input::new(&self.search).focus_bordered(false))
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("exempt-select-visible")
                            .label(tr!("Select matching"))
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.selection.set_visible_checked(true);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("exempt-deselect-visible")
                            .label(tr!("Deselect matching"))
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.selection.set_visible_checked(false);
                                cx.notify();
                            })),
                    ),
            )
            .child(div().text_xs().text_color(muted).child(counts))
            .child(if no_matches {
                div()
                    .h(px(240.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(muted)
                    .child(if empty {
                        tr!("Load JSON to list available fields")
                    } else {
                        tr!("No matching fields")
                    })
                    .into_any_element()
            } else {
                v_flex()
                    .id("exemption-fields-scroll")
                    .relative()
                    .h(px(240.))
                    .min_h_0()
                    .child(list)
                    .vertical_scrollbar(&self.scroll)
                    .into_any_element()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_are_recursive_unique_literal_keys_from_both_sides() {
        let mut fields = BTreeSet::new();
        collect_field_names(
            &serde_json::json!({"root":{"key1":1,"key2":{"key1":2}},"list":[{"名字":1,"":2}],"a,b":1," a ":1}),
            &mut fields,
        );
        collect_field_names(
            &serde_json::json!({"only_b":true,"literal":"not_a_key","0":0}),
            &mut fields,
        );
        assert_eq!(
            fields,
            [
                "", " a ", "0", "a,b", "key1", "key2", "list", "literal", "only_b", "root", "名字"
            ]
            .into_iter()
            .map(str::to_owned)
            .collect()
        );
    }

    #[test]
    fn search_is_case_insensitive_but_selection_remains_exact() {
        let mut selection = FieldSelection::new(
            ["Key1", "key1", "key10", "名字"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            BTreeSet::new(),
        );
        selection.filter(" KEY1 ");
        assert_eq!(selection.visible.len(), 3);
        selection.set_checked(0, true);
        assert!(selection.selected.contains("Key1"));
        assert!(!selection.selected.contains("key1"));
        selection.filter("名字");
        assert_eq!(selection.visible.len(), 1);
        assert!(selection.selected.contains("Key1"));
    }

    #[test]
    fn bulk_selection_only_changes_matching_fields() {
        let fields = ["key1", "key2", "other"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let mut selection = FieldSelection::new(fields, BTreeSet::from(["other".to_string()]));
        selection.filter("key");
        selection.set_visible_checked(true);
        assert_eq!(selection.selected.len(), 3);
        selection.filter("key1");
        selection.set_visible_checked(false);
        assert_eq!(
            selection.selected,
            BTreeSet::from(["key2".to_string(), "other".to_string()])
        );
        selection.filter("missing");
        selection.set_visible_checked(false);
        assert_eq!(selection.selected.len(), 2);
        selection.filter("");
        assert_eq!(selection.visible.len(), 3);
    }

    #[test]
    fn old_exemptions_stay_available_after_source_reload() {
        let selection = FieldSelection::new(
            BTreeSet::from(["new".to_string()]),
            BTreeSet::from(["old".to_string()]),
        );
        assert_eq!(selection.fields, vec!["new", "old"]);
        assert!(selection.selected.contains("old"));
    }
}
