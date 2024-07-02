use crate::app::App;
use chrono::DateTime;
use chrono::Datelike;
use chrono::Local;
use chrono::Timelike;
use chrono::Weekday;
use std::cell::Cell;
use std::cell::RefCell;
use std::ops::RangeInclusive;
use std::rc::Rc;
use x11rb::protocol::xproto::ButtonIndex;
use x11rb::protocol::xproto::CreateWindowAux;
use x11rb::protocol::xproto::EventMask;
use x11rb::protocol::Event;

pub const PANEL_HEIGHT: u16 = 28;

pub struct TopPanel {
    app: Rc<App>,
    id: u32,
    surface: cairo::XCBSurface,
    need_redraw: Cell<bool>,
    time: Cell<DateTime<Local>>,

    // Information about where (on x coordinate) clickable text is drawn
    // We calculate it as we draw and use when handling MotionNotify or ButtonPress
    layout: RefCell<Vec<RangeInclusive<u16>>>,

    // When we receive MotionNotify events, we have to defer their handling
    // So we keep info about the event happening, also with its x coordinate
    // Also note that we only care about the latest MotionNotify event
    deferred_motion_notify_x: Cell<Option<u16>>,

    // Same as for 'deferred_motion_notify_x'
    deferred_click_x: Cell<Option<u16>>,
}

impl TopPanel {
    pub fn new(app: Rc<App>) -> Self {
        let id = app.api().generate_id();

        app.api().create_window(
            id,
            0,
            0,
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
            time: Cell::new(Local::now()),
            layout: RefCell::new(Vec::new()),
            deferred_motion_notify_x: Cell::new(None),
            deferred_click_x: Cell::new(None),
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    fn redraw(&self) {
        if !self.need_redraw.take() {
            return;
        }

        let context = cairo::Context::new(&self.surface).unwrap();

        context.set_line_width(1.0);
        context.set_antialias(cairo::Antialias::None);

        context.set_operator(cairo::Operator::Source);
        context.set_source_rgba(0.0, 0.0, 0.0, 0.8);
        context.paint().unwrap();
        context.set_operator(cairo::Operator::Over);

        self.draw_workspace_labels(&context);
        self.draw_clock(&context);

        self.surface.flush();
    }

    fn draw_workspace_labels(&self, context: &cairo::Context) {
        let workspaces = self.app.wm().workspaces();
        let active_workspace_index = self.app.wm().active_workspace_index();

        context.select_font_face(
            "PxPlus ToshibaTxL2 8x16",
            cairo::FontSlant::Normal,
            cairo::FontWeight::Bold,
        );

        context.set_font_size(18.0);

        let mut layout = self.layout.borrow_mut();
        layout.clear();

        let mut offset = 10;

        for (index, workspace) in workspaces.iter().enumerate() {
            let label = match workspace.tasklist().first() {
                Some(client) => match client.class().as_deref() {
                    Some(class) => format!("[{}]", class.to_uppercase()),
                    None => format!("[{}]", index + 1),
                },
                None => format!("[{}]", index + 1),
            };

            let extents = context.text_extents(&label).unwrap();

            context.move_to(
                offset as _,
                (PANEL_HEIGHT as f64 + extents.height() / 1.5) / 2.0,
            );

            if index == active_workspace_index {
                context.set_source_rgb(0.58, 0.61, 0.64);
            }
            else {
                context.set_source_rgb(0.27, 0.27, 0.27);
            }

            context.show_text(&label).unwrap();

            let start = offset;
            let width = extents.width().round() as u16;
            let end = start + width;

            layout.push(start..=end);

            offset = end + 30;
        }
    }

    fn draw_clock(&self, context: &cairo::Context) {
        context.set_font_size(16.0);

        context.select_font_face(
            "PxPlus ToshibaTxL2 8x16",
            cairo::FontSlant::Normal,
            cairo::FontWeight::Bold,
        );

        context.set_source_rgb(0.58, 0.61, 0.64);

        let time = self.time.get();

        let weekday = match time.weekday() {
            Weekday::Mon => "Monday",
            Weekday::Tue => "Tuesday",
            Weekday::Wed => "Wednesday",
            Weekday::Thu => "Thursday",
            Weekday::Fri => "Friday",
            Weekday::Sat => "Saturday",
            Weekday::Sun => "Sunday",
        };

        let text = format!(
            "{:02}:{:02} // {} {:02}.{:02}.{}",
            time.hour(),
            time.minute(),
            weekday,
            time.day(),
            time.month(),
            time.year()
        );

        let extents = context.text_extents(&text).unwrap();

        context.move_to(
            (self.app.api().screen_width() - 12) as f64 - extents.width(),
            PANEL_HEIGHT as f64 / 2.0 - extents.y_bearing() / 2.25,
        );

        context.show_text(&text).unwrap();
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

    fn handle_click(&self, mouse_x: u16) {
        let workspace_index = self
            .layout
            .borrow()
            .iter()
            .position(|range| range.contains(&mouse_x));

        if let Some(index) = workspace_index {
            self.app.wm().change_active_workspace(index);
        }
    }

    pub fn request_redraw(&self) {
        let time = Local::now();

        if self.time.get() != time {
            self.time.set(time);
            self.need_redraw.set(true);
        }

        self.redraw();

        if let Some(mouse_x) = self.deferred_motion_notify_x.get() {
            self.set_cursor(mouse_x);
        }

        if let Some(mouse_x) = self.deferred_click_x.take() {
            self.handle_click(mouse_x);
        }

        // After we have handled the events we might need to redraw again
        self.redraw();
    }

    pub fn handle_event(&self, event: &Event) {
        match event {
            Event::MotionNotify(event) => {
                if event.event == self.id {
                    self.deferred_motion_notify_x.set(Some(event.event_x as _));
                }
                else {
                    self.deferred_motion_notify_x.set(None);
                }
            }
            Event::ButtonPress(event) => {
                if event.event == self.id && ButtonIndex::from(event.detail) == ButtonIndex::M1 {
                    self.deferred_click_x.set(Some(event.event_x as _));
                }
            }
            _ => {}
        }
    }

    pub fn notify(&self) {
        self.need_redraw.set(true);
    }
}
