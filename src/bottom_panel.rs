use crate::api::ICON_SIZE;
use crate::app::App;
use std::cell::Cell;
use std::cell::RefCell;
use std::ops::RangeInclusive;
use std::rc::Rc;
use x11rb::protocol::xproto::ButtonIndex;
use x11rb::protocol::xproto::ButtonPressEvent;
use x11rb::protocol::xproto::CreateWindowAux;
use x11rb::protocol::xproto::EventMask;
use x11rb::protocol::Event;

pub const PANEL_HEIGHT: u16 = 30;
const ICON_MARGIN_LEFT: u16 = 7;
const ICON_MARGIN_RIGHT: u16 = 10;

pub struct BottomPanel {
    app: Rc<App>,
    id: u32,
    surface: cairo::XCBSurface,
    need_redraw: Cell<bool>,

    // Same as for TopPanel
    layout: RefCell<Vec<RangeInclusive<u16>>>,
    last_mouse_x: Cell<Option<u16>>,
}

impl BottomPanel {
    pub fn new(app: Rc<App>) -> Self {
        let id = app.api().generate_id();

        app.api().create_window(
            id,
            0,
            (app.api().screen_height() - PANEL_HEIGHT) as _,
            app.api().screen_width(),
            PANEL_HEIGHT,
            CreateWindowAux::new().event_mask(EventMask::BUTTON_PRESS | EventMask::POINTER_MOTION),
        );

        app.api().map_window(id);

        let surface =
            app.api()
                .create_cairo_xcb_surface(id, app.api().screen_width(), PANEL_HEIGHT);

        Self {
            app,
            id,
            surface,
            need_redraw: Cell::new(true),
            layout: RefCell::new(Vec::new()),
            last_mouse_x: Cell::new(None),
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn request_redraw(&self) {
        if !self.need_redraw.take() {
            return;
        }

        self.draw();

        if let Some(mouse_x) = self.last_mouse_x.get() {
            self.set_cursor(mouse_x);
        }
    }

    fn draw(&self) {
        let mut layout = self.layout.borrow_mut();
        layout.clear();

        let context = cairo::Context::new(&self.surface).unwrap();

        context.set_line_width(1.0);
        context.set_antialias(cairo::Antialias::None);

        context.set_source_rgb(0.0, 0.0, 0.0);
        context.paint().unwrap();

        let workspace = self.app.wm().active_workspace();
        let clients = workspace.tasklist();

        if clients.is_empty() {
            return;
        }

        context.set_font_size(16.0);

        context.select_font_face(
            "PxPlus ToshibaTxL2 8x16",
            cairo::FontSlant::Normal,
            cairo::FontWeight::Bold,
        );

        let entry_width = self.app.api().screen_width() / clients.len() as u16;

        let (entry_width, justified) = if entry_width > 300 {
            (300, false)
        }
        else {
            (entry_width, true)
        };

        // TODO investigate what this means
        let max_len = ((entry_width - ICON_MARGIN_LEFT - ICON_SIZE - ICON_MARGIN_RIGHT) / 9)
            .saturating_sub(3);

        let active_client_id = workspace.stack().last().unwrap().id();

        for (index, client) in clients.iter().enumerate() {
            let offset = index as u16 * entry_width;
            let is_active = client.id() == active_client_id;
            let is_last = index == clients.len() - 1;

            let width = if justified && is_last {
                self.app.api().screen_width() - entry_width
            }
            else {
                entry_width
            };

            layout.push(offset..=(offset + width));

            if is_active {
                context.set_source_rgb(0.14, 0.14, 0.14);
                context.rectangle(offset as _, 0.0, width as _, PANEL_HEIGHT as _);
                context.fill().unwrap();
            }

            context
                .set_source_surface(
                    client
                        .icon()
                        .as_deref()
                        .unwrap_or(&self.app.api().default_icon),
                    (offset + ICON_MARGIN_LEFT) as _,
                    (PANEL_HEIGHT - ICON_SIZE) as f64 / 2.0,
                )
                .unwrap();

            context.source().set_filter(cairo::Filter::Nearest);
            context.paint().unwrap();

            let title = client
                .title()
                .as_deref()
                .map(|title| {
                    let mut result = String::new();

                    for (index, char) in title.chars().enumerate() {
                        if index == max_len as usize {
                            result.push_str("...");
                            break;
                        }

                        result.push(char);
                    }

                    result
                })
                .unwrap_or_else(|| format!("[{}]", client.id()));

            let extents = context.text_extents(&title).unwrap();

            context.move_to(
                (offset + ICON_MARGIN_LEFT + ICON_SIZE + ICON_MARGIN_RIGHT) as _,
                (PANEL_HEIGHT as f64 / 2.0 - extents.y_bearing() / 2.0).floor(),
            );

            if is_active {
                context.set_source_rgb(0.58, 0.61, 0.64);
            }
            else {
                context.set_source_rgb(0.27, 0.27, 0.27);
            }

            context.show_text(&title).unwrap();
        }

        self.surface.flush();
    }

    fn set_cursor(&self, mouse_x: u16) {
        let mouse_on_clickable_text = self
            .layout
            .borrow()
            .iter()
            .any(|range| range.contains(&mouse_x));

        let cursor = if mouse_on_clickable_text {
            self.app.api().cursors.hand
        }
        else {
            self.app.api().cursors.left_ptr
        };

        self.app.api().set_window_cursor(self.id, cursor);
    }

    fn handle_button_press(&self, event: &ButtonPressEvent) {
        let tasklist_index = self
            .layout
            .borrow()
            .iter()
            .position(|range| range.contains(&(event.root_x as _)));

        if let Some(tasklist_index) = tasklist_index {
            let client_id = self.app.wm().active_workspace().tasklist()[tasklist_index].id();

            let stack_index = self
                .app
                .wm()
                .active_workspace()
                .stack()
                .iter()
                .position(|client| client.id() == client_id)
                .unwrap();

            self.app.wm().raise_client(stack_index);
        }
    }

    pub fn handle_event(&self, event: &Event) {
        match event {
            Event::MotionNotify(event) => {
                if event.event == self.id {
                    self.set_cursor(event.event_x as _);
                    self.last_mouse_x.set(Some(event.event_x as _));
                }
                else {
                    self.last_mouse_x.set(None);
                }
            }
            Event::ButtonPress(event) => {
                if event.event == self.id && ButtonIndex::from(event.detail) == ButtonIndex::M1 {
                    self.handle_button_press(event);
                }
            }
            _ => {}
        }
    }

    pub fn notify(&self) {
        self.need_redraw.set(true);
    }
}
