//! What the pointer does to the app: a press is routed to its owner, held
//! until it is let go, and then acted on; menus and dialogs answer it.
//!
//! What each cell of the frame belongs to is `crate::pointer`'s; this is what
//! `App` does about it. It is a child of `app` so that it can reach the
//! app's own fields, which nothing outside it should.

use super::*;

/// What a menu item does. Most are a key's action on the menu's target, so
/// choosing one runs the same code that key runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MenuAction {
    ZoomPane(PaneId),
    MovePane(PaneId, isize),
    ClosePane(PaneId),
    NewPaneIn(ProjectId),
    FoldProject(ProjectId),
    RemoveProject(ProjectId),
    /// A tab by its place on the row and, when the project keeps them, its
    /// id: the row may have changed since the menu opened.
    RenameTab(usize, Option<TabId>),
    MoveTab(usize, Option<TabId>, isize),
    CloseTab(usize, Option<TabId>),
}

impl App {
    /// Resolves a mouse event against what the last frame drew, and acts on
    /// it when it belongs to the tab row or the sidebar. Returns whether the
    /// event was consumed; when it was not, the router over `layout` handles
    /// it.
    pub(super) fn route_pointer(&mut self, mouse: &MouseEvent, area: Size) -> Result<bool> {
        use crossterm::event::MouseButton;

        // A press still held belongs to what it pressed, wherever the pointer
        // has gone since.
        if let Some(gesture) = self.gesture {
            match mouse.kind {
                MouseEventKind::Drag(button) if button == gesture.button => {
                    self.gesture = Some(pointer::Gesture {
                        last: (mouse.column, mouse.row),
                        ..gesture
                    });
                    self.drag_gesture(gesture, mouse);
                    return Ok(true);
                }
                MouseEventKind::Up(button) if button == gesture.button => {
                    self.gesture = None;
                    self.release_gesture(gesture, mouse, area)?;
                    return Ok(true);
                }
                // Anything else mid-gesture is noise from a terminal that
                // lost a release; the gesture ends as if it had arrived.
                MouseEventKind::Down(_) => self.end_gesture(),
                _ => return Ok(true),
            }
        } else if matches!(mouse.kind, MouseEventKind::Drag(_) | MouseEventKind::Up(_)) {
            // Motion with a button held, or its release, whose press this
            // window never saw (an overlay took it, or the gesture ended
            // early): nothing owns it, and the pane under the pointer did not
            // receive the press.
            return Ok(true);
        }

        // A frame drawn under another overlay knows nothing of this one: what
        // it recorded is not what is on screen, and the layout beneath it may
        // be covered. Nothing acts until the next frame has drawn.
        if !self.hits.is_for(self.overlay_tag()) {
            return Ok(true);
        }

        let target = self
            .hits
            .resolve(mouse.column, mouse.row, self.overlay_tag());

        if matches!(self.overlay, Some(Overlay::Menu(_))) {
            self.route_menu_pointer(mouse, target);
            return Ok(true);
        }
        if self.overlay.is_some() {
            self.route_dialog_pointer(mouse, target, area)?;
            return Ok(true);
        }

        // The drawer lies over the panes: a click beside it puts it away.
        if self.drawer_open
            && matches!(mouse.kind, MouseEventKind::Down(_))
            && !self
                .sidebar_area
                .contains(ratatui::layout::Position::new(mouse.column, mouse.row))
        {
            self.drawer_open = false;
            return Ok(true);
        }

        if let MouseEventKind::Down(button) = mouse.kind
            && let Some(target) = target
        {
            self.gesture = Some(pointer::Gesture {
                owner: target,
                button,
                last: (mouse.column, mouse.row),
                rect: match target {
                    pointer::Target::PaneContent(id) => self
                        .layout
                        .iter()
                        .find(|(pane, _)| *pane == id)
                        .map(|(_, rect)| *rect),
                    _ => None,
                },
            });
            self.pressed_sidebar = None;
            // Whether this press made a double-click, for a header to zoom on
            // the release that follows.
            self.pressed_double =
                button == MouseButton::Left && self.clicks.press(target, self.now());
            match target {
                pointer::Target::Sidebar => {
                    self.pressed_sidebar = sidebar::hit_test(
                        &self.state,
                        self.sidebar_area,
                        &self.sidebar_scroll,
                        mouse.column,
                        mouse.row,
                    );
                    return Ok(true);
                }
                // The child gets its press as it always did, after the pane
                // has the keyboard; everything it is sent after is this
                // gesture's. A child not tracking the mouse is sent nothing.
                pointer::Target::PaneContent(id) => {
                    if button == MouseButton::Left && self.state.focused_pane() != Some(id) {
                        self.focus_pane(id);
                    }
                    return Ok(false);
                }
                _ => return Ok(true),
            }
        }

        // The wheel over the sidebar scrolls the section under it; the grid
        // never sees it, since no pane is under the pointer.
        if matches!(
            mouse.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) && matches!(
            target,
            Some(pointer::Target::Sidebar | pointer::Target::SidebarEdge)
        ) && let Some(device) = sidebar::section_at(
            &self.state,
            self.sidebar_area,
            &self.sidebar_scroll,
            // The edge is the frame's own column, which belongs to no
            // section: its rows are the column beside it.
            if target == Some(pointer::Target::SidebarEdge) {
                mouse.column.saturating_sub(1)
            } else {
                mouse.column
            },
            mouse.row,
        ) {
            let offset = self.sidebar_scroll.entry(device).or_insert(0);
            *offset = if mouse.kind == MouseEventKind::ScrollUp {
                offset.saturating_sub(1)
            } else {
                offset.saturating_add(1)
            };
            return Ok(true);
        }

        // The drawer hides the panes beneath it, so a pointer event on its
        // blank space or border is nobody's: it must not reach them.
        Ok(self.drawer_open && target == Some(pointer::Target::Sidebar))
    }

    /// A pointer event while a menu is open: hover follows the pointer, a
    /// press on an item is that item's until released, and a press anywhere
    /// else closes the menu and goes no further.
    fn route_menu_pointer(&mut self, mouse: &MouseEvent, target: Option<pointer::Target>) {
        let item = match target {
            Some(pointer::Target::Menu(pointer::MenuHit::Item(index))) => Some(index),
            _ => None,
        };
        match mouse.kind {
            MouseEventKind::Moved => {
                if let Some(Overlay::Menu(menu)) = &mut self.overlay {
                    menu.hover(item);
                }
            }
            MouseEventKind::Down(button) => match target {
                Some(owner @ pointer::Target::Menu(pointer::MenuHit::Item(_))) => {
                    self.gesture = Some(pointer::Gesture {
                        owner,
                        button,
                        last: (mouse.column, mouse.row),
                        rect: None,
                    });
                    self.pressed_double = false;
                }
                // The box's frame: the press is the menu's, and does nothing.
                Some(pointer::Target::Menu(pointer::MenuHit::Area)) => {}
                // Outside it, or over a frame drawn before it opened.
                _ => self.close_menu(),
            },
            _ => {}
        }
    }

    /// A pointer event while a dialog is open. It is the dialog's whatever
    /// it lands on, and nothing is passed on to the frame beneath.
    ///
    /// A press on a row selects it at once, and a second one chooses it as
    /// Enter would; a press on a button or an arrow is held until released.
    /// A press outside the box, or on its frame, does nothing.
    fn route_dialog_pointer(
        &mut self,
        mouse: &MouseEvent,
        target: Option<pointer::Target>,
        area: Size,
    ) -> Result<()> {
        use crossterm::event::MouseButton;

        let hit = match target {
            Some(pointer::Target::Dialog(hit)) => Some(hit),
            _ => None,
        };
        match mouse.kind {
            MouseEventKind::Moved => self.hover_dialog_row(match hit {
                Some(pointer::DialogHit::Row(index)) => Some(index),
                _ => None,
            }),
            MouseEventKind::Down(MouseButton::Left) => match (hit, target) {
                (Some(pointer::DialogHit::Row(index)), Some(target)) => {
                    let double = self.clicks.press(target, self.now());
                    self.select_dialog_row(index);
                    if double {
                        self.choose_dialog_row(area)?;
                    }
                }
                (
                    Some(pointer::DialogHit::Button(_) | pointer::DialogHit::Step(..)),
                    Some(owner),
                ) => {
                    // The second press of a double on an approval button is
                    // ignored: answering advances to the next request, which
                    // is drawn in the same box, so the second click would
                    // decide a request the user never read.
                    let double = self.clicks.press(owner, self.now());
                    if double && matches!(self.overlay, Some(Overlay::Approval { .. })) {
                        return Ok(());
                    }
                    self.gesture = Some(pointer::Gesture {
                        owner,
                        button: MouseButton::Left,
                        last: (mouse.column, mouse.row),
                        rect: None,
                    });
                    self.pressed_double = false;
                    if let pointer::Target::Dialog(pointer::DialogHit::Button(id)) = owner {
                        self.set_dialog_pressed(Some(id));
                    }
                }
                _ => {}
            },
            // One notch is one row, inside the box; the panes never see it.
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown if hit.is_some() => {
                let down = mouse.kind == MouseEventKind::ScrollDown;
                match &mut self.overlay {
                    Some(Overlay::Browse(browser)) => {
                        if down {
                            browser.next();
                        } else {
                            browser.previous();
                        }
                    }
                    Some(Overlay::Approval { .. }) => self.scroll_approval(down),
                    Some(Overlay::Settings { form, .. }) => form.scroll_rows(down),
                    Some(overlay) => {
                        if let Some(picker) = overlay.picker_mut() {
                            if down {
                                picker.next();
                            } else {
                                picker.previous();
                            }
                        }
                    }
                    None => {}
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Marks the row the pointer is over, or none.
    fn hover_dialog_row(&mut self, row: Option<usize>) {
        match &mut self.overlay {
            Some(Overlay::Browse(browser)) => browser.set_hovered(row),
            Some(Overlay::Settings { form, .. }) => form.set_hovered(row),
            Some(overlay) => {
                if let Some(picker) = overlay.picker_mut() {
                    picker.set_hovered(row);
                }
            }
            None => {}
        }
    }

    /// Moves the open dialog's selection to the row drawn at `index`.
    fn select_dialog_row(&mut self, index: usize) {
        match &mut self.overlay {
            Some(Overlay::Browse(browser)) => browser.select_visible(index),
            Some(Overlay::Settings { form, .. }) => form.select_row(index),
            Some(overlay) => {
                if let Some(picker) = overlay.picker_mut() {
                    picker.select_shown(index);
                }
            }
            None => {}
        }
    }

    /// What a double-click on a row does: what Enter does on it. The
    /// settings form has no such thing, and a second click there is one more
    /// click.
    fn choose_dialog_row(&mut self, area: Size) -> Result<()> {
        match &self.overlay {
            Some(Overlay::Browse(_)) => {
                self.open_browser_selection();
                Ok(())
            }
            Some(Overlay::Help(_)) => {
                self.run_help_selection();
                Ok(())
            }
            Some(overlay) if overlay.picker().is_some() => self.choose_selected(area),
            _ => Ok(()),
        }
    }

    /// Marks the button held down, or none, on whichever dialog is open.
    fn set_dialog_pressed(&mut self, pressed: Option<pointer::ButtonId>) {
        match &mut self.overlay {
            Some(Overlay::Browse(browser)) => browser.set_pressed(pressed),
            Some(Overlay::Settings { form, .. }) => form.set_pressed(pressed),
            Some(
                Overlay::OpenOn { prompt, .. }
                | Overlay::RenameTab { prompt, .. }
                | Overlay::CloseTab { prompt, .. },
            ) => prompt.set_pressed(pressed),
            Some(Overlay::AddMachine(add)) => add.prompt_mut().set_pressed(pressed),
            Some(Overlay::Approval { .. }) => self.approval_pressed = pressed,
            Some(overlay) => {
                if let Some(picker) = overlay.picker_mut() {
                    picker.set_pressed(pressed);
                }
            }
            None => {}
        }
    }

    /// Where the open dialog drew its rows and buttons, as the last frame
    /// left them: the layout its own render followed. A menu has its own.
    pub(super) fn dialog_layout(&self) -> Option<dispatch_tui::button::DialogLayout> {
        let area = self.overlay_area;
        match self.overlay.as_ref()? {
            Overlay::Settings { form, .. } => Some(form.layout(area)),
            Overlay::Browse(browser) => Some(browser.layout(area)),
            Overlay::OpenOn { prompt, .. }
            | Overlay::RenameTab { prompt, .. }
            | Overlay::CloseTab { prompt, .. } => Some(prompt.layout(area)),
            Overlay::AddMachine(add) => Some(add.prompt().layout(area)),
            Overlay::Approval { scroll } => Some(self.approval_widget(*scroll)?.layout(area)),
            Overlay::Menu(_) => None,
            overlay => overlay.picker().map(|picker| picker.layout(area)),
        }
    }

    /// Does what a dialog's button says: what its key does, by the same
    /// function. A button the open dialog does not have does nothing.
    fn press_button(&mut self, id: pointer::ButtonId, area: Size) -> Result<()> {
        use pointer::ButtonId;

        let Some(overlay) = &self.overlay else {
            return Ok(());
        };
        match (overlay, id) {
            // Every dialog's way out.
            (_, ButtonId::Cancel) | (Overlay::Approval { .. }, ButtonId::Later) => {
                self.cancel_dialog();
            }
            (Overlay::Help(_), ButtonId::Run) => self.run_help_selection(),
            (Overlay::Browse(_), ButtonId::Open) => self.open_browser_selection(),
            (Overlay::Settings { .. }, ButtonId::OpenPane) => {
                self.settings_action(FormAction::Open, area)?;
            }
            (Overlay::Settings { .. }, ButtonId::SaveDefault) => {
                self.settings_action(FormAction::Save, area)?;
            }
            (Overlay::Approval { .. }, ButtonId::Approve) => self.decide(true, false),
            (Overlay::Approval { .. }, ButtonId::Deny) => self.decide(false, false),
            (Overlay::Approval { .. }, ButtonId::Always) => self.decide(true, true),
            (Overlay::OpenOn { .. }, ButtonId::Ok) => self.confirm_open_on(),
            (Overlay::RenameTab { .. }, ButtonId::Ok) => self.confirm_rename_tab(),
            (Overlay::AddMachine(_), ButtonId::Ok) => {
                self.handle_add_machine_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            }
            (Overlay::CloseTab { .. }, ButtonId::Close) => self.confirm_close_tab(),
            (overlay, ButtonId::Open) if overlay.picker().is_some() => {
                self.choose_selected(area)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Puts a menu away, and opens the approval that was waiting behind it.
    pub(super) fn close_menu(&mut self) {
        self.overlay = None;
        self.open_next_approval();
    }

    /// A held press moved: what its owner does with the motion.
    fn drag_gesture(&mut self, gesture: pointer::Gesture, mouse: &MouseEvent) {
        match gesture.owner {
            // The sidebar's right edge is a handle: pressed, dragged, let go.
            // Only the left button moves it, like every Dispatch control.
            pointer::Target::SidebarEdge
                if gesture.button == crossterm::event::MouseButton::Left =>
            {
                use dispatch_config::ui_state::{MAX_SIDEBAR, MIN_SIDEBAR};
                self.sidebar_width = (mouse.column + 1)
                    .saturating_sub(self.sidebar_area.x)
                    .clamp(MIN_SIDEBAR, MAX_SIDEBAR);
            }
            pointer::Target::PaneContent(id) => self.send_mouse_to(
                id,
                gesture.rect,
                (mouse.column, mouse.row),
                dispatch_pty::MouseAction::Motion,
                mouse.kind,
                mouse.modifiers,
            ),
            // A button shows held only while the pointer is over it: sliding
            // off is how a press is called back, and sliding on again holds
            // it once more.
            pointer::Target::Dialog(pointer::DialogHit::Button(id)) => {
                let over = self
                    .hits
                    .resolve(mouse.column, mouse.row, self.overlay_tag())
                    == Some(gesture.owner);
                self.set_dialog_pressed(over.then_some(id));
            }
            _ => {}
        }
    }

    /// A held press was let go: what its owner does with the release.
    fn release_gesture(
        &mut self,
        gesture: pointer::Gesture,
        mouse: &MouseEvent,
        area: Size,
    ) -> Result<()> {
        let owner = gesture.owner;
        // A button held down is let go, whether or not it acts.
        if matches!(owner, pointer::Target::Dialog(_)) {
            self.set_dialog_pressed(None);
        }
        match owner {
            pointer::Target::SidebarEdge => {
                if gesture.button == crossterm::event::MouseButton::Left {
                    // The second click of a double is the handle's way back
                    // to its default, and what is saved is that width rather
                    // than one the press may have dragged to.
                    if self.pressed_double {
                        self.sidebar_width = dispatch_config::ui_state::DEFAULT_SIDEBAR;
                    }
                    self.save_ui();
                }
            }
            pointer::Target::PaneContent(id) => self.send_mouse_to(
                id,
                gesture.rect,
                (mouse.column, mouse.row),
                dispatch_pty::MouseAction::Release,
                mouse.kind,
                mouse.modifiers,
            ),
            // A control acts on a release that is still inside it, and only
            // for the left button: sliding off is how a press is cancelled.
            pointer::Target::Sidebar
            | pointer::Target::Tab(_)
            | pointer::Target::PaneHeader(_)
            | pointer::Target::PaneMenu(_)
            | pointer::Target::Menu(_)
            | pointer::Target::Dialog(_) => {
                let still_on = self
                    .hits
                    .resolve(mouse.column, mouse.row, self.overlay_tag())
                    == Some(owner);
                if still_on && mouse.kind == MouseEventKind::Up(crossterm::event::MouseButton::Left)
                {
                    self.activate(owner, mouse, area)?;
                } else if still_on
                    && mouse.kind == MouseEventKind::Up(crossterm::event::MouseButton::Right)
                {
                    self.open_menu_on(owner, mouse);
                }
            }
        }
        Ok(())
    }

    /// Does what a control does when it is clicked.
    fn activate(&mut self, owner: pointer::Target, mouse: &MouseEvent, area: Size) -> Result<()> {
        match owner {
            pointer::Target::Tab(hit) => match hit {
                TabHit::Tab(index) => self.select_tab(index),
                TabHit::New => self.open_new_tab_picker(),
                TabHit::Previous => self.select_previous_tab(),
                TabHit::Next => self.select_tab(self.current_tab() + 1),
            },
            // The sidebar is not part of input routing — `layout` covers only
            // the tiled grid — so a click on one of its rows is resolved here
            // rather than through the router.
            pointer::Target::Sidebar => {
                let hit = sidebar::hit_test(
                    &self.state,
                    self.sidebar_area,
                    &self.sidebar_scroll,
                    mouse.column,
                    mouse.row,
                );
                let Some(hit) = hit.filter(|hit| Some(*hit) == self.pressed_sidebar) else {
                    return Ok(());
                };
                match hit {
                    sidebar::Hit::Device(id) => self.state.toggle_device_collapsed(id),
                    // A heading carries no pane, so the click moves the view
                    // to the project; folding it is the chevron's alone.
                    sidebar::Hit::Project(id) => self.select_project(id),
                    sidebar::Hit::ProjectChevron(id) => self.state.toggle_project_collapsed(id),
                    sidebar::Hit::Twisty(id) => self.state.toggle_pane_collapsed(id),
                    sidebar::Hit::Pane(id) => self.focus_pane(id),
                }
                // Picking something is what the drawer was opened for.
                if matches!(hit, sidebar::Hit::Project(_) | sidebar::Hit::Pane(_)) {
                    self.drawer_open = false;
                }
            }
            // The header is Dispatch's own: the click gives the pane the
            // keyboard and nothing reaches the child. The second click of a
            // double zooms it, once it has the keyboard.
            pointer::Target::PaneHeader(id) => {
                self.focus_pane(id);
                if self.pressed_double {
                    self.state.toggle_zoom();
                }
            }
            // Under the `…`, where the menu hangs.
            pointer::Target::PaneMenu(id) => {
                self.open_pane_menu(id, (mouse.column, mouse.row.saturating_add(1)));
            }
            // An item acts on its release, like any control.
            pointer::Target::Menu(pointer::MenuHit::Item(index)) => {
                let action = match &self.overlay {
                    Some(Overlay::Menu(menu)) => menu
                        .items()
                        .get(index)
                        .filter(|item| item.enabled)
                        .map(|item| item.action),
                    _ => None,
                };
                if let Some(action) = action {
                    self.choose_menu(action);
                }
            }
            // A row acted when it was pressed. A button or an arrow acts on
            // its release, through what its key runs.
            pointer::Target::Dialog(pointer::DialogHit::Button(id)) => {
                self.press_button(id, area)?;
            }
            pointer::Target::Dialog(pointer::DialogHit::Step(index, forward)) => {
                if let Some(Overlay::Settings { form, .. }) = &mut self.overlay {
                    let action = form.step_row(index, forward);
                    self.settings_action(action, area)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Opens the menu a right-click on `owner` asks for, if it has one.
    ///
    /// The row that was pressed is the one the release is still on, as for a
    /// click, so sliding off it cancels.
    fn open_menu_on(&mut self, owner: pointer::Target, mouse: &MouseEvent) {
        let at = (mouse.column, mouse.row);
        match owner {
            pointer::Target::Sidebar => {
                let hit = sidebar::hit_test(
                    &self.state,
                    self.sidebar_area,
                    &self.sidebar_scroll,
                    mouse.column,
                    mouse.row,
                );
                match hit.filter(|hit| Some(*hit) == self.pressed_sidebar) {
                    Some(sidebar::Hit::Pane(id) | sidebar::Hit::Twisty(id)) => {
                        self.open_pane_menu(id, at);
                    }
                    Some(sidebar::Hit::Project(id) | sidebar::Hit::ProjectChevron(id)) => {
                        self.open_project_menu(id, at);
                    }
                    Some(sidebar::Hit::Device(_)) | None => {}
                }
            }
            pointer::Target::Tab(TabHit::Tab(index)) => {
                self.open_tab_menu(index, (mouse.column, mouse.row.saturating_add(1)));
            }
            pointer::Target::PaneHeader(id) => self.open_pane_menu(id, at),
            // Under the `…`, where its left click hangs the same menu.
            pointer::Target::PaneMenu(id) => {
                self.open_pane_menu(id, (mouse.column, mouse.row.saturating_add(1)));
            }
            _ => {}
        }
    }

    /// The items of a menu, each with the keys that do the same.
    fn menu_item(
        &self,
        label: impl Into<String>,
        command: Option<Command>,
        action: MenuAction,
        enabled: bool,
    ) -> MenuItem<MenuAction> {
        MenuItem {
            label: label.into(),
            keys: command.and_then(|command| self.router.keymap().path_to(command)),
            action,
            enabled,
        }
    }

    /// Opens the menu for a pane at `anchor`, changing nothing else.
    pub(super) fn open_pane_menu(&mut self, id: PaneId, anchor: (u16, u16)) {
        let Some(pane) = self.state.pane(id) else {
            return;
        };
        let (project, shell) = (pane.project, pane.harness.as_str() == SHELL);
        // Moving left from the first tab has nowhere to go. Moving right from
        // the last makes a tab, so it is always open.
        let on_first = tabs::views(&self.state, Some(project), &self.tileable_in(Some(project)))
            .first()
            .is_some_and(|view| view.panes.contains(&id));
        let zoomed = self.state.zoomed_pane() == Some(id);
        let items = vec![
            self.menu_item(
                if zoomed { "Restore" } else { "Zoom" },
                Some(Command::Zoom),
                MenuAction::ZoomPane(id),
                true,
            ),
            self.menu_item(
                "Move to previous tab",
                Some(Command::MovePaneLeft),
                MenuAction::MovePane(id, -1),
                !on_first,
            ),
            self.menu_item(
                "Move to next tab",
                Some(Command::MovePaneRight),
                MenuAction::MovePane(id, 1),
                true,
            ),
            self.menu_item(
                if shell {
                    "Close pane and end shell"
                } else {
                    "Close pane and stop agent"
                },
                Some(Command::ClosePane),
                MenuAction::ClosePane(id),
                true,
            ),
        ];
        self.overlay = Some(Overlay::Menu(Menu::new(items, anchor)));
    }

    /// Opens the menu for a project at `anchor`, changing nothing else.
    pub(super) fn open_project_menu(&mut self, id: ProjectId, anchor: (u16, u16)) {
        if !self.state.projects().iter().any(|project| project.id == id) {
            return;
        }
        let items = vec![
            self.menu_item(
                "New pane here",
                Some(Command::NewPane),
                MenuAction::NewPaneIn(id),
                true,
            ),
            self.menu_item(
                if self.state.is_project_collapsed(id) {
                    "Unfold"
                } else {
                    "Fold"
                },
                Some(Command::Fold),
                MenuAction::FoldProject(id),
                true,
            ),
            // Dropping a project with panes is refused, so it is not offered.
            self.menu_item(
                "Remove from list",
                None,
                MenuAction::RemoveProject(id),
                self.state.panes_for(id).is_empty(),
            ),
        ];
        self.overlay = Some(Overlay::Menu(Menu::new(items, anchor)));
    }

    /// Opens the menu for the tab at `index` at `anchor`, changing nothing
    /// else.
    pub(super) fn open_tab_menu(&mut self, index: usize, anchor: (u16, u16)) {
        let views = self.tab_views();
        let count = views.len();
        let Some(id) = views.get(index).map(|view| view.id) else {
            return;
        };
        let items = vec![
            self.menu_item(
                "Rename",
                Some(Command::RenameTab),
                MenuAction::RenameTab(index, id),
                true,
            ),
            self.menu_item(
                "Move left",
                Some(Command::MoveTabLeft),
                MenuAction::MoveTab(index, id, -1),
                index > 0,
            ),
            self.menu_item(
                "Move right",
                Some(Command::MoveTabRight),
                MenuAction::MoveTab(index, id, 1),
                index + 1 < count,
            ),
            self.menu_item(
                "Close tab",
                Some(Command::CloseTab),
                MenuAction::CloseTab(index, id),
                true,
            ),
        ];
        self.overlay = Some(Overlay::Menu(Menu::new(items, anchor)));
    }

    /// Brings a menu's pane into view and gives it the focus, which may mean
    /// selecting its project. Says whether it is the focus now: a pane that
    /// closed while the menu was open is not, and nothing then acts on
    /// whichever pane is.
    fn go_to_menu_pane(&mut self, id: PaneId) -> bool {
        self.go_to_pane(id);
        if self.state.focused_pane() == Some(id) {
            return true;
        }
        self.warn("that pane is gone");
        false
    }

    /// Whether a menu's project is still listed. One dropped while the menu
    /// was open is not, and nothing then acts on whichever project is
    /// selected.
    fn project_is_there(&mut self, id: ProjectId) -> bool {
        if self.state.projects().iter().any(|project| project.id == id) {
            return true;
        }
        self.warn("that project is gone");
        false
    }

    /// Shows a menu's tab, if the tab at `index` is still the one it was
    /// opened on. `select_tab` would wrap a stale index to the first tab.
    fn select_menu_tab(&mut self, index: usize, id: Option<TabId>) -> bool {
        if self.tab_views().get(index).map(|view| view.id) != Some(id) {
            self.warn("that tab is gone");
            return false;
        }
        self.select_tab(index);
        true
    }

    /// Closes the menu and does what its chosen item says, by giving its
    /// target the focus or selection and running what its key runs.
    pub(super) fn choose_menu(&mut self, action: MenuAction) {
        self.overlay = None;
        // Choosing is what the drawer was opened for, as picking a row is.
        self.drawer_open = false;
        match action {
            MenuAction::ZoomPane(id) => {
                if !self.go_to_menu_pane(id) {
                    return;
                }
                // Another pane being zoomed would be restored by the toggle
                // instead of this one being zoomed.
                if self.state.zoomed_pane().is_some_and(|zoomed| zoomed != id) {
                    self.state.toggle_zoom();
                }
                self.state.toggle_zoom();
            }
            MenuAction::MovePane(id, by) => {
                if self.go_to_menu_pane(id) {
                    self.move_focused_pane(by);
                }
            }
            MenuAction::ClosePane(id) => self.close_pane(id),
            MenuAction::NewPaneIn(project) => {
                if self.project_is_there(project) {
                    self.select_project(project);
                    self.open_harness_picker();
                }
            }
            MenuAction::FoldProject(project) => {
                if self.project_is_there(project) {
                    self.select_project(project);
                    self.state.toggle_project_collapsed(project);
                }
            }
            MenuAction::RemoveProject(project) => {
                if self.project_is_there(project) {
                    self.drop_project(project);
                }
            }
            MenuAction::RenameTab(index, id) => {
                if self.select_menu_tab(index, id) {
                    self.open_rename_tab();
                }
            }
            MenuAction::MoveTab(index, id, by) => {
                if self.select_menu_tab(index, id) {
                    self.move_current_tab(by);
                }
            }
            MenuAction::CloseTab(index, id) => {
                if self.select_menu_tab(index, id) {
                    self.open_close_tab();
                }
            }
        }
        if self.overlay.is_none() {
            self.open_next_approval();
        }
    }

    /// Ends a press on the frame, which an open overlay has taken the pointer
    /// from. One on the overlay itself is left for its release.
    pub(super) fn end_gesture_beneath_overlay(&mut self) {
        if self
            .gesture
            .is_some_and(|gesture| !gesture.owner.is_overlay())
        {
            self.end_gesture();
        }
    }

    /// Ends a press whose owner can no longer take its release, sending that
    /// release first to a pane that can, so no child keeps a stuck button.
    pub(super) fn end_gesture(&mut self) {
        let Some(gesture) = self.gesture.take() else {
            return;
        };
        if matches!(gesture.owner, pointer::Target::Dialog(_)) {
            self.set_dialog_pressed(None);
        }
        // Any pane still alive is sent its release, in or out of the grid: the
        // rectangle it was pressed in stands in for one it no longer has.
        if let pointer::Target::PaneContent(id) = gesture.owner
            && self.panes.contains_key(&id)
        {
            self.send_mouse_to(
                id,
                gesture.rect,
                gesture.last,
                dispatch_pty::MouseAction::Release,
                MouseEventKind::Up(gesture.button),
                KeyModifiers::NONE,
            );
        }
        if gesture.owner == pointer::Target::SidebarEdge
            && gesture.button == crossterm::event::MouseButton::Left
        {
            self.save_ui();
        }
    }

    /// Forwards a pointer event at window cell `at` to `id`, relative to that
    /// pane's interior however far outside it the pointer has gone.
    fn send_mouse_to(
        &mut self,
        id: PaneId,
        pressed_in: Option<Rect>,
        at: (u16, u16),
        action: dispatch_pty::MouseAction,
        kind: MouseEventKind,
        modifiers: KeyModifiers,
    ) {
        use crossterm::event::MouseButton as Pressed;
        use dispatch_pty::MouseButton;

        let (MouseEventKind::Down(button)
        | MouseEventKind::Up(button)
        | MouseEventKind::Drag(button)) = kind
        else {
            return;
        };
        let Some(rect) = self
            .layout
            .iter()
            .find(|(pane, _)| *pane == id)
            .map(|(_, rect)| *rect)
            .or(pressed_in)
        else {
            return;
        };
        let button = match button {
            Pressed::Left => MouseButton::Left,
            Pressed::Middle => MouseButton::Middle,
            Pressed::Right => MouseButton::Right,
        };
        self.send_mouse(
            id,
            MouseInput {
                action,
                button,
                col: at.0.saturating_sub(rect.x),
                row: at.1.saturating_sub(rect.y),
                modifiers: dispatch_pty::Modifiers {
                    shift: modifiers.contains(KeyModifiers::SHIFT),
                    ctrl: modifiers.contains(KeyModifiers::CONTROL),
                    alt: modifiers.contains(KeyModifiers::ALT),
                    super_: modifiers.contains(KeyModifiers::SUPER),
                },
            },
        );
    }
}
