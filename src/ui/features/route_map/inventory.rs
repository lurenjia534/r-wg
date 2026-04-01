use gpui::prelude::FluentBuilder as _;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use gpui::*;
use gpui_component::{
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    list::ListItem,
    tag::Tag,
    tree::{tree, TreeEntry, TreeItem, TreeState},
    ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _, StyledExt as _,
};

use crate::ui::state::WgApp;

use super::data::{
    RouteMapChip, RouteMapData, RouteMapInventoryGroup, RouteMapInventoryItem, RouteMapItemStatus,
    RouteMapTone,
};
use super::status_chip;

struct RouteMapInventoryTreeCache {
    tree: Entity<TreeState>,
    rows: Arc<HashMap<SharedString, InventoryTreeRow>>,
    signature: u64,
    selected_ix: Option<usize>,
}

#[derive(Clone)]
struct InventoryTreeRow {
    kind: InventoryTreeRowKind,
}

#[derive(Clone)]
enum InventoryTreeRowKind {
    Group {
        label: SharedString,
        count: SharedString,
        first_child_id: Option<SharedString>,
    },
    Item {
        title: SharedString,
        status: RouteMapItemStatus,
        chips: Vec<RouteMapChip>,
        monospace: bool,
        explain_matched: bool,
    },
}

struct InventoryTreeModel {
    signature: u64,
    tree_items: Vec<TreeItem>,
    rows: HashMap<SharedString, InventoryTreeRow>,
    selected_ix: Option<usize>,
}

impl RouteMapInventoryTreeCache {
    fn new(cx: &mut App) -> Self {
        Self {
            tree: cx.new(|cx| TreeState::new(cx)),
            rows: Arc::new(HashMap::new()),
            signature: 0,
            selected_ix: None,
        }
    }

    fn sync(&mut self, model: &RouteMapData, cx: &mut App) {
        let next = build_inventory_tree_model(model);
        if self.signature != next.signature {
            self.signature = next.signature;
            self.rows = Arc::new(next.rows);
            let items = next.tree_items;
            self.tree.update(cx, |tree, cx| {
                tree.set_items(items, cx);
            });
        }

        if self.selected_ix != next.selected_ix {
            self.selected_ix = next.selected_ix;
            let selected_ix = self.selected_ix;
            self.tree.update(cx, |tree, cx| {
                tree.set_selected_index(selected_ix, cx);
                if let Some(ix) = selected_ix {
                    tree.scroll_to_item(ix, ScrollStrategy::Top);
                }
            });
        }
    }
}

pub(crate) fn render_inventory(
    app: &mut WgApp,
    model: &RouteMapData,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> Div {
    let content_style = StyleRefinement::default().flex_grow().min_h(px(0.0));
    let content = if !model.has_plan {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .min_h(px(0.0))
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(model.plan_status.clone()),
            )
            .when_some(model.parse_error.as_ref(), |this, parse_error| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(parse_error.clone()),
                )
            })
            .into_any_element()
    } else {
        let tree_cache = window.use_keyed_state("route-map-inventory-tree", cx, |_, cx| {
            RouteMapInventoryTreeCache::new(cx)
        });
        tree_cache.update(cx, |state, cx| {
            state.sync(model, cx);
        });

        let app_handle = cx.entity();
        let rows = tree_cache.read(cx).rows.clone();
        let tree_state = tree_cache.read(cx).tree.clone();

        tree(
            &tree_state,
            move |ix, entry: &TreeEntry, selected, _window, cx| {
                let row = rows
                    .get(&entry.item().id)
                    .expect("inventory tree row should exist");
                render_tree_row(&app_handle, ix, entry, row, selected, cx)
            },
        )
        .size_full()
        .into_any_element()
    };

    let _ = app;

    div()
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .h_full()
        .min_h(px(0.0))
        .child(
            GroupBox::new()
                .fill()
                .flex_grow()
                .content_style(content_style)
                .title("Inventory")
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .w_full()
                        .h_full()
                        .min_h(px(0.0))
                        .child(content),
                ),
        )
}

fn render_tree_row(
    app: &Entity<WgApp>,
    ix: usize,
    entry: &TreeEntry,
    row: &InventoryTreeRow,
    selected: bool,
    cx: &mut App,
) -> ListItem {
    match &row.kind {
        InventoryTreeRowKind::Group {
            label,
            count,
            first_child_id,
        } => {
            sync_tree_selection(app, selected, first_child_id.clone(), cx);
            let depth_padding = px(12.0 + entry.depth() as f32 * 14.0);
            ListItem::new(ix).selected(selected).child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .pl(depth_padding)
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Icon::new(if entry.is_expanded() {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .size_3()
                                .text_color(cx.theme().muted_foreground),
                            )
                            .child(div().text_sm().font_semibold().child(label.clone())),
                    )
                    .child(Tag::secondary().small().rounded_full().child(count.clone())),
            )
        }
        InventoryTreeRowKind::Item {
            title,
            status,
            chips,
            monospace,
            explain_matched,
        } => {
            let item_id = entry.item().id.clone();
            let app_handle = app.clone();
            sync_tree_selection(app, selected, Some(item_id.clone()), cx);
            let depth_padding = px(18.0 + entry.depth() as f32 * 14.0);
            let title_row = if *monospace {
                div()
                    .text_sm()
                    .font_semibold()
                    .truncate()
                    .font_family(cx.theme().mono_font_family.clone())
                    .child(title.clone())
            } else {
                div()
                    .text_sm()
                    .font_semibold()
                    .truncate()
                    .child(title.clone())
            };

            let leading_chip = chips.first().map(compact_chip);
            ListItem::new(ix)
                .selected(selected)
                .secondary_selected(*explain_matched && !selected)
                .on_click(move |_, _, cx| {
                    app_handle.update(cx, |app, cx| {
                        app.set_route_map_selected_item(Some(item_id.clone()), cx);
                    });
                })
                .child(
                    h_flex()
                        .w_full()
                        .pl(depth_padding)
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(div().flex_1().min_w(px(0.0)).truncate().child(title_row))
                        .child(
                            h_flex()
                                .items_center()
                                .gap_1()
                                .when(*explain_matched, |this| {
                                    this.child(Tag::info().xsmall().rounded_full().child("Explain"))
                                })
                                .when_some(leading_chip, |this, chip| this.child(chip))
                                .child(status_chip(*status)),
                        ),
                )
        }
    }
}

fn sync_tree_selection(
    app: &Entity<WgApp>,
    selected: bool,
    target_item_id: Option<SharedString>,
    cx: &mut App,
) {
    if !selected {
        return;
    }

    let Some(target_item_id) = target_item_id else {
        return;
    };
    let current = app.read(cx).ui_session.route_map_selected_item.clone();
    if current.as_ref() == Some(&target_item_id) {
        return;
    }

    let app_handle = app.clone();
    cx.defer(move |cx| {
        app_handle.update(cx, |app, cx| {
            app.set_route_map_selected_item(Some(target_item_id.clone()), cx);
        });
    });
}

fn compact_chip(chip: &RouteMapChip) -> Tag {
    match chip.tone {
        RouteMapTone::Secondary => Tag::secondary()
            .xsmall()
            .rounded_full()
            .child(chip.label.clone()),
        RouteMapTone::Info => Tag::info()
            .xsmall()
            .rounded_full()
            .child(chip.label.clone()),
        RouteMapTone::Warning => Tag::warning()
            .xsmall()
            .rounded_full()
            .child(chip.label.clone()),
    }
}

fn build_inventory_tree_model(model: &RouteMapData) -> InventoryTreeModel {
    let mut hasher = DefaultHasher::new();
    let mut rows = HashMap::new();
    let mut tree_items = Vec::new();
    let mut selected_ix = None;
    let mut group_offset = 0usize;

    for group in &model.inventory_groups {
        hash_group(group, &mut hasher);
        let expanded = !group.items.is_empty();
        rows.insert(
            group.id.clone(),
            InventoryTreeRow {
                kind: InventoryTreeRowKind::Group {
                    label: group.label.clone(),
                    count: group.summary.clone(),
                    first_child_id: group.items.first().map(|item| item.id.clone()),
                },
            },
        );

        let children = group
            .items
            .iter()
            .enumerate()
            .map(|(item_ix, item)| {
                hash_item(item, &mut hasher);
                rows.insert(
                    item.id.clone(),
                    InventoryTreeRow {
                        kind: InventoryTreeRowKind::Item {
                            title: item.title.clone(),
                            status: item.status,
                            chips: item.chips.clone(),
                            monospace: item.route_row.is_some()
                                || item.title.as_ref().contains('/')
                                || item.title.as_ref().contains(':'),
                            explain_matched: model.explain_match_id.as_ref() == Some(&item.id),
                        },
                    },
                );
                if model.selected_item_id.as_ref() == Some(&item.id) {
                    selected_ix = Some(group_offset + item_ix + 1);
                }
                TreeItem::new(item.id.clone(), item.title.clone())
            })
            .collect::<Vec<_>>();

        tree_items.push(
            TreeItem::new(group.id.clone(), group.label.clone())
                .expanded(expanded)
                .children(children),
        );
        group_offset += group.items.len() + 1;
    }

    InventoryTreeModel {
        signature: hasher.finish(),
        tree_items,
        rows,
        selected_ix,
    }
}

fn hash_group(group: &RouteMapInventoryGroup, hasher: &mut DefaultHasher) {
    group.id.hash(hasher);
    group.label.hash(hasher);
    group.summary.hash(hasher);
    group.empty_note.hash(hasher);
}

fn hash_item(item: &RouteMapInventoryItem, hasher: &mut DefaultHasher) {
    item.id.hash(hasher);
    item.title.hash(hasher);
    item.subtitle.hash(hasher);
    item.status.hash(hasher);
    for chip in &item.chips {
        chip.label.hash(hasher);
    }
    if let Some(route_row) = item.route_row.as_ref() {
        route_row.note.hash(hasher);
    }
}
