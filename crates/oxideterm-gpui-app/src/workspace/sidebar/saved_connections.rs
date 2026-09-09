use super::super::session_manager::{
    SessionManagerDisplayItem, SessionManagerInput, SessionManagerItemPointerAction,
    SessionManagerOpenTarget, SessionManagerRowActionTarget, SessionManagerSelectionTarget,
    SessionManagerTreeRow, collect_session_group_paths, group_display_name,
    session_manager_item_pointer_action, session_manager_tree_rows,
};
use super::*;
use std::{
    collections::HashSet,
    hash::{DefaultHasher, Hash},
};

// Compact saved-connections navigator. It shares the full manager tab's
// display model (filter/sort/grouping/open flows) but owns its search query,
// single selection, and menu state so the tab's batch selection is untouched.
#[derive(Clone, Debug)]
pub(in crate::workspace) struct SavedSidebarMenu {
    target: SessionManagerRowActionTarget,
}

impl WorkspaceApp {
    pub(in crate::workspace) fn render_saved_connections_sidebar_content(
        &mut self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let query = self.session_manager.read(cx).sidebar_search_query.clone();
        let items = Arc::<[SessionManagerDisplayItem]>::from(self.saved_sidebar_display_items(cx));
        // A deleted profile must not keep a ghost selection behind.
        if let Some(selected) = self.saved_sidebar_selected.clone()
            && !items
                .iter()
                .any(|item| item.selection_target() == Some(selected.clone()))
        {
            self.saved_sidebar_selected = None;
        }
        if let Some(menu) = self.saved_sidebar_menu.clone()
            && items
                .iter()
                .all(|item| item.row_action_target() != Some(menu.target.clone()))
        {
            self.saved_sidebar_menu = None;
        }

        let (roots, children) = self.session_group_tree();
        // The sidebar defaults every group to expanded and tracks only the
        // collapsed ones locally, leaving the manager tab's expansion alone.
        // Searching always flattens the tree regardless of collapsed state.
        let mut visible_expanded = HashSet::new();
        collect_session_group_paths(&roots, &children, &mut visible_expanded);
        if query.trim().is_empty() {
            for collapsed in &self.saved_sidebar_collapsed {
                visible_expanded.remove(collapsed);
            }
        }
        let rows = Arc::<[SessionManagerTreeRow]>::from(session_manager_tree_rows(
            &items,
            &roots,
            &children,
            &visible_expanded,
        ));
        // Live-surface identities back the connected indicator below.
        let connected = Arc::new(self.active_saved_connection_ids(cx));
        self.sync_saved_sidebar_list_state(&rows, &items, &connected, cx);
        let state = self.saved_sidebar_list_state.clone();
        let spec = TauriVirtualListSpec::new(
            px(SAVED_SIDEBAR_LIST_ESTIMATED_HEIGHT),
            SAVED_SIDEBAR_LIST_OVERSCAN,
        );
        let workspace = cx.entity();
        let search_value = query.clone();
        div()
            .id("saved-connections-sidebar")
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .flex()
            .flex_col()
            .pt(px(PRIMARY_SIDEBAR_CONTENT_TOP_INSET))
            .child(
                div()
                    .flex_none()
                    .w_full()
                    .px_2()
                    .pb_2()
                    .child(self.render_session_text_input(
                        SessionManagerInput::SidebarSearch,
                        &search_value,
                        self.i18n.t("sessionManager.toolbar.search_placeholder"),
                        cx,
                    )),
            )
            .child(
                div()
                    .id("saved-connections-sidebar-scroll")
                    .flex_1()
                    .min_h(px(0.0))
                    .w_full()
                    .child(if rows.is_empty() {
                        self.render_saved_sidebar_empty(cx).into_any_element()
                    } else {
                        tauri_virtual_list(state, spec, move |index, _window, cx| {
                            workspace.update(cx, |this, cx| {
                                this.render_saved_sidebar_row(
                                    rows.get(index),
                                    &items,
                                    &connected,
                                    cx,
                                )
                            })
                        })
                        .into_any_element()
                    }),
            )
            .into_any_element()
    }

    fn render_saved_sidebar_empty(&self, _cx: &mut Context<Self>) -> AnyElement {
        let theme = self.tokens.ui;
        div()
            .flex_1()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .p_4()
            .child(Self::render_lucide_icon(
                LucideIcon::Inbox,
                SESSION_TREE_ICON_SIZE * 2.0,
                rgb(theme.text_muted),
            ))
            .child(
                div()
                    .text_size(px(SESSION_TREE_TEXT_SIZE))
                    .text_color(rgb(theme.text_muted))
                    .child(self.i18n.t("sidebar.panels.no_saved_connections")),
            )
            .child(
                div()
                    .text_size(px(SESSION_TREE_META_TEXT_SIZE))
                    .text_color(rgb(theme.text_muted))
                    .child(self.i18n.t("sidebar.add_new_connection")),
            )
            .into_any_element()
    }

    fn sync_saved_sidebar_list_state(
        &mut self,
        rows: &[SessionManagerTreeRow],
        items: &[SessionManagerDisplayItem],
        connected: &HashSet<String>,
        _cx: &App,
    ) {
        let selected = self.saved_sidebar_selected.clone();
        let menu_target = self.saved_sidebar_menu.clone().map(|menu| menu.target);
        let signatures = rows
            .iter()
            .map(|row| {
                let mut hasher = DefaultHasher::new();
                // Row height changes when the inline menu opens beneath it;
                // hash selection and menu ownership alongside row identity.
                match row {
                    SessionManagerTreeRow::Group { path, expanded, .. } => {
                        path.hash(&mut hasher);
                        expanded.hash(&mut hasher);
                    }
                    SessionManagerTreeRow::Item { item_index, .. } => {
                        if let Some(item) = items.get(*item_index) {
                            item.id().hash(&mut hasher);
                            item.name().hash(&mut hasher);
                            item.subtitle().hash(&mut hasher);
                            (item.selection_target() == selected).hash(&mut hasher);
                            saved_sidebar_item_connected(item, connected).hash(&mut hasher);
                            menu_target
                                .as_ref()
                                .is_some_and(|target| {
                                    item.row_action_target().as_ref() == Some(target)
                                })
                                .hash(&mut hasher);
                        } else {
                            item_index.hash(&mut hasher);
                        }
                    }
                }
                std::hash::Hasher::finish(&hasher)
            })
            .collect::<Vec<_>>();
        sync_tauri_variable_list_state_by_signatures(
            &self.saved_sidebar_list_state,
            &mut self.saved_sidebar_list_cache.borrow_mut(),
            "saved-connections-sidebar",
            &signatures,
            TauriVirtualListSpec::new(
                px(SAVED_SIDEBAR_LIST_ESTIMATED_HEIGHT),
                SAVED_SIDEBAR_LIST_OVERSCAN,
            ),
        );
    }

    fn render_saved_sidebar_row(
        &self,
        row: Option<&SessionManagerTreeRow>,
        items: &[SessionManagerDisplayItem],
        connected: &HashSet<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match row {
            Some(SessionManagerTreeRow::Group {
                path,
                depth,
                expanded,
                ..
            }) => self
                .render_saved_sidebar_group_row(path, *depth, *expanded, items, cx)
                .into_any_element(),
            Some(SessionManagerTreeRow::Item { item_index, depth }) => items
                .get(*item_index)
                .map(|item| self.render_saved_sidebar_item_row(item, *depth, connected, cx))
                .unwrap_or_else(|| div().into_any_element()),
            None => div().into_any_element(),
        }
    }

    fn render_saved_sidebar_group_row(
        &self,
        group: &str,
        depth: usize,
        expanded: bool,
        items: &[SessionManagerDisplayItem],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = self.tokens.ui;
        let group_id = group.to_string();
        let item_count = saved_sidebar_group_item_count(items, group);
        div()
            .w_full()
            .h(px(SESSION_TREE_NODE_HEIGHT))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .px_2()
            .pl(px(depth as f32 * 16.0 + 8.0))
            .rounded(px(self.tokens.radii.md))
            .cursor_pointer()
            .hover(|row| row.bg(rgb(theme.bg_hover)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event, _window, cx| {
                    if !this.saved_sidebar_collapsed.remove(&group_id) {
                        this.saved_sidebar_collapsed.insert(group_id.clone());
                    }
                    cx.notify();
                    // Keep the click from reaching the root outside-pointer
                    // dismissal or a terminal behind the sidebar.
                    cx.stop_propagation();
                }),
            )
            .child(self.render_animated_chevron(
                gpui::SharedString::from(format!("saved-sidebar-group-{group}")),
                expanded,
                SESSION_TREE_CHILD_ICON_SIZE,
                rgb(theme.text_muted),
            ))
            .child(Self::render_lucide_icon(
                if expanded {
                    LucideIcon::FolderOpen
                } else {
                    LucideIcon::Folder
                },
                SESSION_TREE_ICON_SIZE,
                rgb(theme.text_muted),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(px(SESSION_TREE_TEXT_SIZE))
                    .text_color(rgb(theme.text))
                    .child(group_display_name(group)),
            )
            .when(item_count > 0, |row| {
                row.child(
                    div()
                        .flex_none()
                        .ml_2()
                        .text_size(px(SESSION_TREE_META_TEXT_SIZE))
                        .text_color(rgb(theme.text_muted))
                        .child(item_count.to_string()),
                )
            })
            .into_any_element()
    }

    fn render_saved_sidebar_item_row(
        &self,
        item: &SessionManagerDisplayItem,
        depth: usize,
        connected: &HashSet<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = self.tokens.ui;
        let selected = item.selection_target() == self.saved_sidebar_selected;
        let item_connected = saved_sidebar_item_connected(item, connected);
        let select_target = item.selection_target();
        let open_target = item.open_target();
        // Each pointer closure owns its copy; the row keeps the originals
        // for the inline menu rendered below.
        let left_open_target = open_target.clone();
        let left_select_target = select_target.clone();
        let menu_target = item.row_action_target();
        let menu_open = menu_target.as_ref().is_some_and(|target| {
            self.saved_sidebar_menu
                .as_ref()
                .is_some_and(|menu| menu.target == *target)
        });
        // Single-line rows mirror the active-sessions node recipe: same
        // height, icon/text sizes, selection chrome, and hover behavior.
        let row = div()
            .w_full()
            .h(px(SESSION_TREE_NODE_HEIGHT))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .rounded(px(self.tokens.radii.md))
            .px_2()
            .pl(px(depth as f32 * 16.0 + 8.0))
            .cursor_pointer()
            .bg(if selected {
                rgba((theme.accent << 8) | SESSION_FOCUS_CARD_SELECTED_BG_ALPHA)
            } else {
                rgba(theme.bg << 8)
            })
            .border_1()
            .border_color(if selected {
                rgba((theme.accent << 8) | SESSION_FOCUS_CARD_SELECTED_BORDER_ALPHA)
            } else {
                rgba(theme.bg << 8)
            })
            .hover(|row| row.bg(rgb(theme.bg_hover)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    match session_manager_item_pointer_action(event.click_count, true) {
                        SessionManagerItemPointerAction::Open => {
                            this.saved_sidebar_menu = None;
                            this.open_session_manager_target(left_open_target.clone(), window, cx);
                        }
                        SessionManagerItemPointerAction::Select => {
                            this.saved_sidebar_menu = None;
                            this.saved_sidebar_selected = left_select_target.clone();
                            cx.notify();
                        }
                        SessionManagerItemPointerAction::None => {}
                    }
                    cx.stop_propagation();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _event, _window, cx| {
                    // Right-click selects the row first so the inline menu
                    // actions always operate on the visible selection.
                    this.saved_sidebar_selected = select_target.clone();
                    this.saved_sidebar_menu = menu_target
                        .clone()
                        .map(|target| SavedSidebarMenu { target });
                    cx.notify();
                    cx.stop_propagation();
                }),
            )
            .child(Self::render_lucide_icon(
                item.icon(),
                SESSION_TREE_ICON_SIZE,
                rgb(if selected { theme.accent } else { theme.text }),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(px(SESSION_TREE_TEXT_SIZE))
                    .font_weight(if selected {
                        gpui::FontWeight::MEDIUM
                    } else {
                        gpui::FontWeight::NORMAL
                    })
                    .text_color(rgb(theme.text))
                    .child(item.name().to_string()),
            )
            .when(item_connected, |row| {
                row.child(self.render_session_status_dot(
                    self.session_node_status(ActiveSessionStatus::Connected),
                ))
            });
        if menu_open && let Some(menu) = self.saved_sidebar_menu.clone() {
            return row
                .child(self.render_saved_sidebar_menu(menu, open_target, cx))
                .into_any_element();
        }
        row.into_any_element()
    }

    fn render_saved_sidebar_menu(
        &self,
        menu: SavedSidebarMenu,
        open_target: SessionManagerOpenTarget,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = self.tokens.ui;
        let open_target = Some(open_target);
        let edit_target = menu.target.clone();
        let connect_label = self.i18n.t("sessionManager.actions.connect");
        let edit_label = self.i18n.t("sessionManager.actions.edit");
        oxideterm_gpui_ui::context_menu::context_menu_event_boundary(
            div()
                .w_full()
                .my(px(4.0))
                .py_1()
                .rounded(px(self.tokens.radii.md))
                .border_1()
                .border_color(rgb(theme.border))
                .bg(rgb(theme.bg_elevated))
                .shadow_lg()
                .child(
                    self.workspace_context_menu_action(
                        div()
                            .w_full()
                            .px_3()
                            .py_1()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(px(SESSION_TREE_TEXT_SIZE))
                            .text_color(rgb(theme.text))
                            .child(Self::render_lucide_icon(
                                LucideIcon::Play,
                                SESSION_TREE_CHILD_ICON_SIZE,
                                rgb(theme.text_muted),
                            ))
                            .child(connect_label),
                        false,
                        false,
                        |this| {
                            this.saved_sidebar_menu = None;
                        },
                        move |this, _event, window, cx| {
                            if let Some(target) = open_target.clone() {
                                this.open_session_manager_target(target, window, cx);
                            }
                        },
                        cx,
                    ),
                )
                .child(
                    self.workspace_context_menu_action(
                        div()
                            .w_full()
                            .px_3()
                            .py_1()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(px(SESSION_TREE_TEXT_SIZE))
                            .text_color(rgb(theme.text))
                            .child(Self::render_lucide_icon(
                                LucideIcon::Pencil,
                                SESSION_TREE_CHILD_ICON_SIZE,
                                rgb(theme.text_muted),
                            ))
                            .child(edit_label),
                        false,
                        false,
                        |this| {
                            this.saved_sidebar_menu = None;
                        },
                        move |this, _event, window, cx| {
                            this.open_saved_sidebar_editor_target(&edit_target, window, cx);
                        },
                        cx,
                    ),
                ),
        )
        .into_any_element()
    }

    fn open_saved_sidebar_editor_target(
        &mut self,
        target: &SessionManagerRowActionTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Reuse the full manager's per-type editors; the sidebar never edits inline.
        match target {
            SessionManagerRowActionTarget::Connection(id) => {
                self.open_saved_connection_editor(id, None, window, cx)
            }
            SessionManagerRowActionTarget::Serial(id) => {
                self.open_saved_serial_profile_editor(id, window, cx)
            }
            SessionManagerRowActionTarget::Telnet(id) => {
                self.open_saved_telnet_profile_editor(id, window, cx)
            }
            SessionManagerRowActionTarget::Mosh(id) => {
                self.open_saved_mosh_profile_editor(id, window, cx)
            }
            SessionManagerRowActionTarget::StandaloneSftp(id) => {
                self.open_saved_standalone_sftp_profile_editor(id, window, cx)
            }
            SessionManagerRowActionTarget::RemoteDesktop(id) => {
                self.open_saved_remote_desktop_profile_editor(id, window, cx)
            }
            SessionManagerRowActionTarget::GroupRoot | SessionManagerRowActionTarget::Group(_) => {}
        }
    }

    pub(in crate::workspace) fn dismiss_saved_sidebar_menu(
        &mut self,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.saved_sidebar_menu.is_none() {
            return false;
        }
        self.saved_sidebar_menu = None;
        cx.notify();
        true
    }
}

/// Whether the profile behind a row currently owns a live surface, backing
/// the same connected indicator the active-sessions navigator paints.
fn saved_sidebar_item_connected(
    item: &SessionManagerDisplayItem,
    connected: &HashSet<String>,
) -> bool {
    let id = match item.selection_target() {
        Some(SessionManagerSelectionTarget::Connection(id))
        | Some(SessionManagerSelectionTarget::Serial(id))
        | Some(SessionManagerSelectionTarget::Telnet(id))
        | Some(SessionManagerSelectionTarget::Mosh(id))
        | Some(SessionManagerSelectionTarget::StandaloneSftp(id))
        | Some(SessionManagerSelectionTarget::RemoteDesktop(id)) => id,
        None => return false,
    };
    connected.contains(&id)
}

/// Profiles in the group subtree, shown as the muted count badge next to
/// the group name.
fn saved_sidebar_group_item_count(items: &[SessionManagerDisplayItem], group: &str) -> usize {
    let prefix = format!("{group}/");
    items
        .iter()
        .filter(|item| {
            item.group()
                .is_some_and(|item_group| item_group == group || item_group.starts_with(&prefix))
        })
        .count()
}
