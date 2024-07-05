use crate::app::App;
use crate::bottom_panel;
use crate::client;
use crate::client::Client;
use crate::keycode::Keycode;
use crate::top_panel;
use crate::util::cycle_next;
use crate::util::cycle_previous;
use nix::unistd::execvp;
use serde::Deserialize;
use serde::Serialize;
use std::cell::Cell;
use std::cell::Ref;
use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::CString;
use std::fs::File;
use std::io::BufReader;
use std::io::BufWriter;
use std::ops::Deref;
use std::rc::Rc;
use x11rb::protocol::xproto::AtomEnum;
use x11rb::protocol::xproto::ButtonIndex;
use x11rb::protocol::xproto::ButtonPressEvent;
use x11rb::protocol::xproto::ConfigWindow;
use x11rb::protocol::xproto::ConfigureRequestEvent;
use x11rb::protocol::xproto::KeyButMask;
use x11rb::protocol::xproto::KeyPressEvent;
use x11rb::protocol::xproto::MapRequestEvent;
use x11rb::protocol::xproto::MapState;
use x11rb::protocol::xproto::ModMask;
use x11rb::protocol::xproto::MotionNotifyEvent;
use x11rb::protocol::xproto::PropertyNotifyEvent;
use x11rb::protocol::xproto::UnmapNotifyEvent;
use x11rb::protocol::Event;

pub struct Wm {
    app: Rc<App>,
    workspaces: [Workspace; 9],
    active_workspace_index: Cell<usize>,
    drag_state: Cell<Option<DragState>>,
}

#[derive(Default)]
pub struct Workspace {
    stack: RefCell<Vec<Rc<Client>>>,
    tasklist: RefCell<Vec<Rc<Client>>>,
}

impl Workspace {
    pub fn stack(&self) -> Ref<Vec<Rc<Client>>> {
        self.stack.borrow()
    }

    pub fn tasklist(&self) -> Ref<Vec<Rc<Client>>> {
        self.tasklist.borrow()
    }
}

#[derive(Clone, Copy)]
struct DragState {
    kind: DragKind,
    x: u16,
    y: u16,
}

#[derive(Clone, Copy)]
enum DragKind {
    Move,
    Resize,
}

#[derive(Serialize, Deserialize, Default)]
struct SerializedState {
    workspaces: [SerializedWorkspace; 9],
    active_workspace_index: usize,
}

#[derive(Serialize, Deserialize, Default, Debug)]
struct SerializedWorkspace {
    stack: Vec<SerializedClient>,
    tasklist: Vec<u32>,
}

#[derive(Serialize, Deserialize, Debug)]
struct SerializedClient {
    id: u32,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
    maximized: bool,
}

enum ExistingClientInfo {
    Id(u32),
    Serialized(SerializedClient),
}

impl Wm {
    pub fn new(app: Rc<App>) -> Self {
        let serialized_state: SerializedState = File::open(get_serialized_state_file_path())
            .ok()
            .and_then(|file| serde_json::from_reader(BufReader::new(file)).ok())
            .unwrap_or_default();

        let this = Self {
            app,
            workspaces: Default::default(),
            active_workspace_index: Cell::new(serialized_state.active_workspace_index),
            drag_state: Cell::new(None),
        };

        this.init(serialized_state.workspaces);

        this
    }

    fn init(&self, serialized_workspaces: [SerializedWorkspace; 9]) {
        let mut existing_client_ids: HashSet<_> = self
            .app
            .api()
            .get_window_children(self.app.api().root())
            .into_iter()
            .collect();

        for ((workspace_index, workspace), serialized_workspace) in self
            .workspaces
            .iter()
            .enumerate()
            .zip(serialized_workspaces)
        {
            for client in serialized_workspace.stack {
                if !existing_client_ids.remove(&client.id) {
                    continue;
                }

                let Some(client) =
                    self.manage_existing_client(ExistingClientInfo::Serialized(client))
                else {
                    continue;
                };

                if workspace_index == self.active_workspace_index() {
                    self.app.api().map_window(client.container_id());
                }

                workspace.stack.borrow_mut().push(Rc::new(client));
            }

            for id in serialized_workspace.tasklist {
                let stack = workspace.stack();

                let Some(client) = stack.iter().find(|client| client.id() == id)
                else {
                    continue;
                };

                workspace.tasklist.borrow_mut().push(client.clone());
            }
        }

        let active_workspace = self.active_workspace();
        let mut active_workspace_stack = active_workspace.stack.borrow_mut();
        let mut active_workspace_tasklist = active_workspace.tasklist.borrow_mut();

        for id in existing_client_ids {
            let Some(client) = self.manage_existing_client(ExistingClientInfo::Id(id))
            else {
                continue;
            };

            self.app.api().map_window(client.container_id());

            let client = Rc::new(client);

            active_workspace_stack.push(client.clone());
            active_workspace_tasklist.push(client);
        }

        self.app
            .api()
            .set_focus(active_workspace_stack.last().map(|client| client.id()));
    }

    fn manage_existing_client(&self, info: ExistingClientInfo) -> Option<Client> {
        let id = match info {
            ExistingClientInfo::Id(id) => id,
            ExistingClientInfo::Serialized(ref client) => client.id,
        };

        let attrs = self.app.api().get_window_attributes(id);

        if attrs.map_state == MapState::UNMAPPED {
            return None;
        }

        if attrs.override_redirect {
            return None;
        }

        let (x, y, width, height, maximized) = match info {
            ExistingClientInfo::Id(id) => {
                let geometry = self.app.api().get_window_geometry(id);

                let maximized = geometry.width == self.app.api().screen_width()
                    && geometry.height
                        == self.app.api().screen_height()
                            - top_panel::PANEL_HEIGHT
                            - bottom_panel::PANEL_HEIGHT;

                (
                    geometry.x,
                    geometry.y,
                    geometry.width,
                    geometry.height,
                    maximized,
                )
            }
            ExistingClientInfo::Serialized(client) => (
                client.x,
                client.y,
                client.width,
                client.height,
                client.maximized,
            ),
        };

        Some(Client::new(
            self.app.clone(),
            id,
            x,
            y,
            width,
            height,
            maximized,
            self.app.api().get_window_class(id),
            self.app.api().get_window_title(id),
            self.app.api().get_window_icon(id),
        ))
    }

    fn handle_map_request(&self, event: &MapRequestEvent) {
        let id = event.window;

        let client_already_managed = self.workspaces.iter().any(|workspace| {
            workspace
                .stack
                .borrow()
                .iter()
                .any(|client| client.id() == id)
        });

        if client_already_managed {
            return;
        }

        let geometry = self.app.api().get_window_geometry(id);

        let maximized_width = self.app.api().screen_width();

        let maximized_height =
            self.app.api().screen_height() - top_panel::PANEL_HEIGHT - bottom_panel::PANEL_HEIGHT;

        let maximized = geometry.width == maximized_width;

        // In particular, this is an issue with VS Code
        if maximized && geometry.height != maximized_height {
            self.app.api().set_window_height(id, maximized_height);
        }

        // We don't know the actual client size when it starts up "maximized",
        // so use a default
        let (width, height) = if maximized {
            (1000, 800)
        }
        else {
            (geometry.width, geometry.height)
        };

        let x = (self.app.api().screen_width() as i16 - width as i16) / 2;
        let y = (self.app.api().screen_height() as i16 + top_panel::PANEL_HEIGHT as i16
            - height as i16)
            / 2;

        let client = Rc::new(Client::new(
            self.app.clone(),
            id,
            x,
            y,
            width,
            height,
            maximized,
            self.app.api().get_window_class(id),
            self.app.api().get_window_title(id),
            self.app.api().get_window_icon(id),
        ));

        self.app.api().map_window(client.id());
        self.app.api().map_window(client.container_id());
        self.app.api().set_focus(client.id());

        let mut stack = self.active_workspace().stack.borrow_mut();
        let mut tasklist = self.active_workspace().tasklist.borrow_mut();

        if let Some(active_client) = stack.last() {
            active_client.notify();

            let tasklist_index = tasklist
                .iter()
                .position(|client| client.id() == active_client.id())
                .unwrap();

            tasklist.insert(tasklist_index + 1, client.clone());
        }
        else {
            tasklist.push(client.clone());
        }

        stack.push(client);

        self.app.api().raise_window(self.app.top_panel().id());
        self.app.api().raise_window(self.app.bottom_panel().id());

        self.app.top_panel().notify();
        self.app.bottom_panel().notify();
    }

    fn handle_unmap_notify(&self, event: &UnmapNotifyEvent) {
        let Some((workspace_index, client_stack_index)) = self
            .workspaces
            .iter()
            .enumerate()
            .find_map(|(workspace_index, workspace)| {
                workspace
                    .stack
                    .borrow()
                    .iter()
                    .position(|client| client.id() == event.window)
                    .map(|client_index| (workspace_index, client_index))
            })
        else {
            return;
        };

        let workspace = &self.workspaces[workspace_index];
        workspace.stack.borrow_mut().remove(client_stack_index);

        let client_tasklist_index = workspace
            .tasklist
            .borrow()
            .iter()
            .position(|client| client.id() == event.window)
            .unwrap();

        workspace
            .tasklist
            .borrow_mut()
            .remove(client_tasklist_index);

        self.app.top_panel().notify();

        if workspace_index == self.active_workspace_index() {
            let stack = workspace.stack.borrow();

            if let Some(client) = stack.last() {
                client.notify();
            }

            self.app
                .api()
                .set_focus(stack.last().map(|client| client.id()));

            self.app.bottom_panel().notify();
        }
    }

    fn handle_key_press(&self, event: &KeyPressEvent) {
        let Ok(keycode) = Keycode::try_from(event.detail)
        else {
            return;
        };

        let is_shift = event.state.contains(ModMask::SHIFT);

        match keycode {
            Keycode::Escape => {
                let file = File::create(get_serialized_state_file_path()).unwrap();
                serde_json::to_writer(BufWriter::new(file), &self.serialize()).unwrap();

                let args = std::env::args()
                    .map(|s| CString::new(s).unwrap())
                    .collect::<Vec<_>>();

                execvp(&args[0], &args).unwrap();
            }
            Keycode::K if is_shift => self.move_active_client_forward_in_tasklist(),
            Keycode::J if is_shift => self.move_active_client_backward_in_tasklist(),
            Keycode::K => self.raise_next_tasklist_client(),
            Keycode::J => self.raise_previous_tasklist_client(),
            Keycode::Number1 if is_shift => self.move_active_client_to_workspace(0),
            Keycode::Number2 if is_shift => self.move_active_client_to_workspace(1),
            Keycode::Number3 if is_shift => self.move_active_client_to_workspace(2),
            Keycode::Number4 if is_shift => self.move_active_client_to_workspace(3),
            Keycode::Number5 if is_shift => self.move_active_client_to_workspace(4),
            Keycode::Number6 if is_shift => self.move_active_client_to_workspace(5),
            Keycode::Number7 if is_shift => self.move_active_client_to_workspace(6),
            Keycode::Number8 if is_shift => self.move_active_client_to_workspace(7),
            Keycode::Number9 if is_shift => self.move_active_client_to_workspace(8),
            Keycode::Number1 => self.change_active_workspace(0),
            Keycode::Number2 => self.change_active_workspace(1),
            Keycode::Number3 => self.change_active_workspace(2),
            Keycode::Number4 => self.change_active_workspace(3),
            Keycode::Number5 => self.change_active_workspace(4),
            Keycode::Number6 => self.change_active_workspace(5),
            Keycode::Number7 => self.change_active_workspace(6),
            Keycode::Number8 => self.change_active_workspace(7),
            Keycode::Number9 => self.change_active_workspace(8),
            Keycode::Right => self.change_active_workspace(cycle_next(
                &self.workspaces,
                self.active_workspace_index(),
            )),
            Keycode::Left => self.change_active_workspace(cycle_previous(
                &self.workspaces,
                self.active_workspace_index(),
            )),
            Keycode::X => {
                if let Some(client) = self.active_workspace().stack().last() {
                    self.app.api().ask_window_to_close(client.id())
                }
            }
            Keycode::M => {
                if let Some(client) = self.active_workspace().stack().last() {
                    client.set_maximized(!client.maximized());
                }
            }
            _ => {}
        }
    }

    fn move_active_client_to_workspace(&self, workspace_index: usize) {
        let mut stack = self.active_workspace().stack.borrow_mut();
        let mut tasklist = self.active_workspace().tasklist.borrow_mut();

        let Some(client) = stack.pop()
        else {
            return;
        };

        self.app.api().unmap_window(client.container_id());

        if let Some(client) = stack.last() {
            client.notify();
        }

        self.app.api().raise_window(client.container_id());
        self.app.api().raise_window(self.app.top_panel().id());
        self.app.api().raise_window(self.app.bottom_panel().id());

        self.app
            .api()
            .set_focus(stack.last().map(|client| client.id()));

        let client_tasklist_index = tasklist.iter().position(|c| c.id() == client.id()).unwrap();

        tasklist.remove(client_tasklist_index);

        let new_workspace = &self.workspaces[workspace_index];
        new_workspace.stack.borrow_mut().push(client.clone());
        new_workspace.tasklist.borrow_mut().push(client);

        self.app.top_panel().notify();
        self.app.bottom_panel().notify();
    }

    fn move_active_client_forward_in_tasklist(&self) {
        let stack = self.active_workspace().stack();

        let Some(active_client) = stack.last()
        else {
            return;
        };

        let mut tasklist = self.active_workspace().tasklist.borrow_mut();

        let client_tasklist_index = tasklist
            .iter()
            .position(|client| client.id() == active_client.id())
            .unwrap();

        let next_client_tasklist_index = cycle_next(&tasklist, client_tasklist_index);

        tasklist.swap(client_tasklist_index, next_client_tasklist_index);

        self.app.top_panel().notify();
        self.app.bottom_panel().notify();
    }

    fn move_active_client_backward_in_tasklist(&self) {
        let stack = self.active_workspace().stack();

        let Some(active_client) = stack.last()
        else {
            return;
        };

        let mut tasklist = self.active_workspace().tasklist.borrow_mut();

        let client_tasklist_index = tasklist
            .iter()
            .position(|client| client.id() == active_client.id())
            .unwrap();

        let previous_client_tasklist_index = cycle_previous(&tasklist, client_tasklist_index);

        tasklist.swap(client_tasklist_index, previous_client_tasklist_index);

        self.app.top_panel().notify();
        self.app.bottom_panel().notify();
    }

    fn raise_next_tasklist_client(&self) {
        let next_client_stack_index = {
            let stack = self.active_workspace().stack();

            let Some(active_client) = stack.last()
            else {
                return;
            };

            let tasklist = self.active_workspace().tasklist();

            let client_tasklist_index = tasklist
                .iter()
                .position(|client| client.id() == active_client.id())
                .unwrap();

            let next_client_tasklist_index = cycle_next(&tasklist, client_tasklist_index);
            let next_client = tasklist[next_client_tasklist_index].deref();

            stack
                .iter()
                .position(|client| client.id() == next_client.id())
                .unwrap()
        };

        self.raise_client(next_client_stack_index);
    }

    fn raise_previous_tasklist_client(&self) {
        let previous_client_stack_index = {
            let stack = self.active_workspace().stack();

            let Some(active_client) = stack.last()
            else {
                return;
            };

            let tasklist = self.active_workspace().tasklist();

            let client_tasklist_index = tasklist
                .iter()
                .position(|client| client.id() == active_client.id())
                .unwrap();

            let previous_client_tasklist_index = cycle_previous(&tasklist, client_tasklist_index);
            let previous_client = tasklist[previous_client_tasklist_index].deref();

            stack
                .iter()
                .position(|client| client.id() == previous_client.id())
                .unwrap()
        };

        self.raise_client(previous_client_stack_index);
    }

    fn handle_button_press(&self, event: &ButtonPressEvent) {
        let clients = self.active_workspace().stack.borrow();

        let Some(client_index) = clients
            .iter()
            .position(|client| client.id() == event.event || client.container_id() == event.event)
        else {
            return;
        };

        let on_container = clients[client_index].container_id() == event.event;
        let button = ButtonIndex::from(event.detail);
        let is_mod4 = event.state.contains(KeyButMask::MOD4);

        if on_container {
            if !(button == ButtonIndex::M1 || (button == ButtonIndex::M3 && is_mod4)) {
                return;
            }
        }
        else {
            self.app.api().allow_pointer_events();
        }

        // raise_client() needs exclusive access to clients so we have to explicitly unlock them
        drop(clients);
        self.raise_client(client_index);
        let clients = self.active_workspace().stack.borrow();
        let client = clients.last().unwrap();

        if client.maximized() {
            return;
        }

        let on_titlebar = (client::BORDER_WIDTH..=(client::BORDER_WIDTH + client.width()))
            .contains(&(event.event_x as _))
            && (client::BORDER_WIDTH..=(client::BORDER_WIDTH + client::TITLEBAR_HEIGHT))
                .contains(&(event.event_y as _));

        match button {
            ButtonIndex::M1 if is_mod4 || (on_container && on_titlebar) => {
                self.drag_state.set(Some(DragState {
                    kind: DragKind::Move,
                    x: event.root_x as _,
                    y: event.root_y as _,
                }));
            }
            ButtonIndex::M3 if is_mod4 => {
                let x = (client.x() + client.width() as i16) as u16;
                let y = (client.y() + client.height() as i16) as u16;

                self.app.api().move_pointer(x, y);

                self.drag_state.set(Some(DragState {
                    kind: DragKind::Resize,
                    x,
                    y,
                }));
            }
            _ => {}
        }
    }

    fn handle_motion_notify(&self, event: &MotionNotifyEvent) {
        let Some(state) = self.drag_state.get()
        else {
            return;
        };

        let clients = self.active_workspace().stack.borrow();

        let Some(client) = clients
            .iter()
            .find(|client| client.id() == event.event || client.container_id() == event.event)
        else {
            return;
        };

        let dx = event.root_x - state.x as i16;
        let dy = event.root_y - state.y as i16;

        match state.kind {
            DragKind::Move => self.handle_drag_move(client, dx, dy),
            DragKind::Resize => self.handle_drag_resize(client, dx, dy),
        }

        self.drag_state.set(Some(DragState {
            kind: state.kind,
            x: event.root_x as _,
            y: event.root_y as _,
        }));
    }

    fn handle_drag_move(&self, client: &Client, dx: i16, dy: i16) {
        client.set_x(client.x() + dx);
        client.set_y(client.y() + dy);
    }

    fn handle_drag_resize(&self, client: &Client, dx: i16, dy: i16) {
        let width = (client.width() as i16 + dx).max(1) as _;
        let height = (client.height() as i16 + dy).max(1) as _;

        client.set_size(width, height);
    }

    fn handle_property_notify(&self, event: &PropertyNotifyEvent) {
        let Some((workspace_index, client_stack_index)) = self
            .workspaces
            .iter()
            .enumerate()
            .find_map(|(workspace_index, workspace)| {
                workspace
                    .stack
                    .borrow()
                    .iter()
                    .position(|client| client.id() == event.window)
                    .map(|client_index| (workspace_index, client_index))
            })
        else {
            return;
        };

        let stack = self.workspaces[workspace_index].stack();
        let client = stack[client_stack_index].deref();

        if event.atom == u32::from(AtomEnum::WM_CLASS) {
            client.set_class(self.app.api().get_window_class(client.id()));
            self.app.top_panel().notify();
        }
        else if event.atom == self.app.api().atoms._NET_WM_NAME {
            client.set_title(self.app.api().get_window_title(client.id()));

            if workspace_index == self.active_workspace_index.get() {
                self.app.bottom_panel().notify();
            }
        }
        else if event.atom == self.app.api().atoms._NET_WM_ICON {
            client.set_icon(self.app.api().get_window_icon(client.id()));

            if workspace_index == self.active_workspace_index.get() {
                self.app.bottom_panel().notify();
            }
        }
    }

    pub fn handle_configure_request(&self, event: &ConfigureRequestEvent) {
        dbg!(event);

        let Some((workspace, client_stack_index)) = self.workspaces.iter().find_map(|workspace| {
            workspace
                .stack
                .borrow()
                .iter()
                .position(|client| client.id() == event.window)
                .map(|client_index| (workspace, client_index))
        })
        else {
            self.app.api().allow_configure_request(event);
            return;
        };

        if !event.value_mask.contains(ConfigWindow::WIDTH)
            && !event.value_mask.contains(ConfigWindow::HEIGHT)
        {
            return;
        }

        let stack = workspace.stack();
        let client = stack[client_stack_index].deref();
        client.set_size(event.width, event.height);
    }

    pub fn change_active_workspace(&self, index: usize) {
        if self.active_workspace_index.get() == index {
            return;
        }

        let workspace = &self.workspaces[index];

        for client in workspace.stack.borrow().iter().rev() {
            self.app.api().map_window(client.container_id());
            client.notify();
        }

        self.app
            .api()
            .set_focus(workspace.stack.borrow().last().map(|client| client.id()));

        for client in self.active_workspace().stack.borrow().iter() {
            self.app.api().unmap_window(client.container_id());
        }

        self.active_workspace_index.set(index);
        self.app.top_panel().notify();
        self.app.bottom_panel().notify();
    }

    pub fn raise_client(&self, stack_index: usize) {
        let mut clients = self.active_workspace().stack.borrow_mut();

        if stack_index == clients.len() - 1 {
            return;
        }

        let client = clients.remove(stack_index);

        if let Some(client) = clients.last() {
            client.notify();
        }

        self.app.api().raise_window(client.container_id());
        self.app.api().raise_window(self.app.top_panel().id());
        self.app.api().raise_window(self.app.bottom_panel().id());
        self.app.api().set_focus(client.id());

        client.notify();
        clients.push(client);

        self.app.bottom_panel().notify();
    }

    pub fn active_workspace_index(&self) -> usize {
        self.active_workspace_index.get()
    }

    pub fn workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }

    pub fn active_workspace(&self) -> &Workspace {
        &self.workspaces[self.active_workspace_index.get()]
    }

    pub fn handle_event(&self, event: &Event) {
        match event {
            Event::MapRequest(event) => self.handle_map_request(event),
            Event::UnmapNotify(event) => self.handle_unmap_notify(event),
            Event::KeyPress(event) => self.handle_key_press(event),
            Event::ButtonPress(event) => self.handle_button_press(event),
            Event::MotionNotify(event) => self.handle_motion_notify(event),
            Event::ButtonRelease(_) => self.drag_state.set(None),
            Event::PropertyNotify(event) => self.handle_property_notify(event),
            Event::ConfigureRequest(event) => self.handle_configure_request(event),
            _ => {}
        }
    }

    pub fn request_redraw(&self) {
        let clients = self.active_workspace().stack.borrow();

        for (index, client) in clients.iter().enumerate() {
            client.request_redraw(index == clients.len() - 1);
        }
    }

    fn serialize(&self) -> SerializedState {
        SerializedState {
            active_workspace_index: self.active_workspace_index(),
            workspaces: self
                .workspaces
                .iter()
                .map(|workspace| SerializedWorkspace {
                    stack: workspace
                        .stack()
                        .iter()
                        .map(|client| SerializedClient {
                            id: client.id(),
                            x: client.x(),
                            y: client.y(),
                            width: client.width(),
                            height: client.height(),
                            maximized: client.maximized(),
                        })
                        .collect(),
                    tasklist: workspace
                        .tasklist()
                        .iter()
                        .map(|client| client.id())
                        .collect(),
                })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        }
    }
}

fn get_serialized_state_file_path() -> String {
    format!("/tmp/vaporwm{}.json", std::env::var("DISPLAY").unwrap())
}
