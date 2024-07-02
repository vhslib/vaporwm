use crate::api::ICON_SIZE;
use crate::app::App;
use crate::bottom_panel;
use crate::top_panel;
use std::borrow::Cow;
use std::cell::Cell;
use std::cell::Ref;
use std::cell::RefCell;
use std::rc::Rc;
use x11rb::protocol::xproto::ButtonIndex;
use x11rb::protocol::xproto::CreateWindowAux;
use x11rb::protocol::xproto::EventMask;
use x11rb::protocol::xproto::GrabMode;
use x11rb::protocol::xproto::ModMask;

pub const BORDER_WIDTH: u16 = 5;
pub const TITLEBAR_HEIGHT: u16 = 25;
const ICON_MARGIN_LEFT: u16 = 7;
const ICON_MARGIN_RIGHT: u16 = 9;

pub struct Client {
    app: Rc<App>,
    id: u32,
    container_id: u32,

    x: Cell<i16>,
    y: Cell<i16>,
    width: Cell<u16>,
    height: Cell<u16>,
    maximized: Cell<bool>,
    class: RefCell<Option<String>>,
    title: RefCell<Option<String>>,
    icon: RefCell<Option<cairo::ImageSurface>>,

    surface: cairo::XCBSurface,
    need_redraw: Cell<bool>,
}

impl Client {
    pub fn new(
        app: Rc<App>,
        id: u32,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        maximized: bool,
        class: Option<String>,
        title: Option<String>,
        icon: Option<cairo::ImageSurface>,
    ) -> Self {
        let container_id = app.api().generate_id();
        let surface = app.api().create_cairo_xcb_surface(container_id, 1, 1);

        let this = Self {
            app,
            id,
            container_id,
            x: Cell::new(x),
            y: Cell::new(y),
            width: Cell::new(width),
            height: Cell::new(height),
            maximized: Cell::new(maximized),
            class: RefCell::new(class),
            title: RefCell::new(title),
            icon: RefCell::new(icon),
            surface,
            need_redraw: Cell::new(true),
        };

        this.init();

        this
    }

    fn init(&self) {
        self.app.api().create_window(
            self.container_id,
            self.container_x(),
            self.container_y(),
            self.container_width(),
            self.container_height(),
            CreateWindowAux::new().event_mask(
                EventMask::SUBSTRUCTURE_REDIRECT
                    | EventMask::SUBSTRUCTURE_NOTIFY
                    | EventMask::BUTTON_PRESS
                    | EventMask::BUTTON_MOTION
                    | EventMask::BUTTON_RELEASE,
            ),
        );

        if !self.maximized() {
            self.grab_buttons_on_container();
        }

        self.app.api().add_to_save_set(self.id);

        self.app.api().reparent_window(
            self.id,
            self.container_id,
            self.inner_offset_x(),
            self.inner_offset_y(),
        );

        self.app.api().grab_button(
            self.id,
            EventMask::BUTTON_PRESS,
            ButtonIndex::M1,
            ModMask::ANY,
            x11rb::NONE,
            true,
            GrabMode::SYNC,
            GrabMode::ASYNC,
            x11rb::NONE,
        );

        self.app.api().grab_button(
            self.id,
            EventMask::BUTTON_PRESS,
            ButtonIndex::M3,
            ModMask::ANY,
            x11rb::NONE,
            true,
            GrabMode::SYNC,
            GrabMode::ASYNC,
            x11rb::NONE,
        );

        self.app
            .api()
            .set_window_event_mask(self.id, EventMask::PROPERTY_CHANGE);

        self.app.api().set_window_border_width(self.id, 0);
        self.app.api().put_wm_state_property(self.id);

        self.surface
            .set_size(self.container_width() as _, self.container_height() as _)
            .unwrap();
    }

    fn container_x(&self) -> i16 {
        if self.maximized() {
            0
        }
        else {
            self.x() - BORDER_WIDTH as i16
        }
    }

    fn container_y(&self) -> i16 {
        if self.maximized() {
            top_panel::PANEL_HEIGHT as _
        }
        else {
            self.y() - BORDER_WIDTH as i16 - TITLEBAR_HEIGHT as i16
        }
    }

    fn container_width(&self) -> u16 {
        if self.maximized() {
            self.app.api().screen_width()
        }
        else {
            self.width() + BORDER_WIDTH * 2
        }
    }

    fn container_height(&self) -> u16 {
        if self.maximized() {
            self.app.api().screen_height() - top_panel::PANEL_HEIGHT - bottom_panel::PANEL_HEIGHT
        }
        else {
            self.height() + BORDER_WIDTH * 2 + TITLEBAR_HEIGHT
        }
    }

    fn inner_offset_x(&self) -> i16 {
        if self.maximized() {
            0
        }
        else {
            BORDER_WIDTH as _
        }
    }

    fn inner_offset_y(&self) -> i16 {
        if self.maximized() {
            0
        }
        else {
            (BORDER_WIDTH + TITLEBAR_HEIGHT) as _
        }
    }

    fn grab_buttons_on_container(&self) {
        self.app.api().grab_button(
            self.container_id,
            EventMask::BUTTON_PRESS | EventMask::BUTTON_MOTION | EventMask::BUTTON_RELEASE,
            ButtonIndex::M1,
            ModMask::M4,
            self.app.api().cursors.fleur,
            false,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            x11rb::NONE,
        );

        self.app.api().grab_button(
            self.container_id,
            EventMask::BUTTON_PRESS | EventMask::BUTTON_MOTION | EventMask::BUTTON_RELEASE,
            ButtonIndex::M3,
            ModMask::M4,
            self.app.api().cursors.bottom_right_corner,
            false,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            x11rb::NONE,
        );
    }

    fn ungrab_buttons_on_container(&self) {
        self.app
            .api()
            .ungrab_button(self.container_id, ButtonIndex::M1, ModMask::M4);

        self.app
            .api()
            .ungrab_button(self.container_id, ButtonIndex::M3, ModMask::M4);
    }

    pub fn request_redraw(&self, is_active: bool) {
        if !self.need_redraw.get() || self.maximized() {
            return;
        }

        self.need_redraw.set(false);

        let context = cairo::Context::new(&self.surface).unwrap();

        context.set_line_width(1.0);
        context.set_antialias(cairo::Antialias::None);

        self.draw_frame(&context);
        self.draw_titlebar(&context, is_active);

        self.surface.flush();
    }

    fn draw_frame(&self, context: &cairo::Context) {
        context.set_source_rgb(0.75, 0.75, 0.75);
        context.paint().unwrap();

        let left = 1.0;
        let right = self.container_width() as f64;
        let top = 1.0;
        let bottom = self.container_height() as f64;

        context.set_source_rgb(1.0, 1.0, 1.0);
        context.move_to(left + 1.0, bottom - 2.0);
        context.line_to(left + 1.0, top + 1.0);
        context.line_to(right - 2.0, top + 1.0);
        context.stroke().unwrap();

        context.set_source_rgb(0.5, 0.5, 0.5);
        context.move_to(left, bottom - 1.0);
        context.line_to(right - 1.0, bottom - 1.0);
        context.line_to(right - 1.0, top);
        context.stroke().unwrap();

        context.set_source_rgb(0.87, 0.87, 0.87);
        context.move_to(left, bottom - 1.0);
        context.line_to(left, top);
        context.line_to(right - 1.0, top);
        context.stroke().unwrap();

        context.set_source_rgb(0.0, 0.0, 0.0);
        context.move_to(left - 1.0, bottom);
        context.line_to(right, bottom);
        context.line_to(right, top - 1.0);
        context.stroke().unwrap();
    }

    fn draw_titlebar(&self, context: &cairo::Context, is_active: bool) {
        let gradient = cairo::LinearGradient::new(0.0, 0.0, self.width() as _, 0.0);

        if is_active {
            gradient.add_color_stop_rgb(0.0, 0.0, 0.5, 0.5);
            gradient.add_color_stop_rgb(1.0, 0.0, 0.67, 0.67);
        }
        else {
            gradient.add_color_stop_rgb(0.0, 0.63, 0.55, 0.4);
            gradient.add_color_stop_rgb(1.0, 0.83, 0.8, 0.73);
        }

        context.set_source(gradient).unwrap();

        context.rectangle(
            BORDER_WIDTH as _,
            BORDER_WIDTH as _,
            self.width() as _,
            TITLEBAR_HEIGHT as _,
        );

        context.fill().unwrap();

        context
            .set_source_surface(
                self.icon
                    .borrow()
                    .as_deref()
                    .unwrap_or(&self.app.api().default_icon),
                (BORDER_WIDTH + ICON_MARGIN_LEFT) as _,
                BORDER_WIDTH as f64 + (TITLEBAR_HEIGHT - ICON_SIZE) as f64 / 2.5,
            )
            .unwrap();

        context.source().set_filter(cairo::Filter::Nearest);
        context.paint().unwrap();

        let maybe_title = self.title();

        let title = maybe_title
            .as_deref()
            .map(Cow::from)
            .unwrap_or_else(|| format!("[{}]", self.id).into());

        context.set_source_rgb(1.0, 1.0, 1.0);

        context.select_font_face(
            "PxPlus ToshibaTxL2 8x16",
            cairo::FontSlant::Normal,
            cairo::FontWeight::Normal,
        );

        context.set_font_size(16.0);

        let extents = context.text_extents(&title).unwrap();

        context.move_to(
            (BORDER_WIDTH + ICON_MARGIN_LEFT + ICON_SIZE + ICON_MARGIN_RIGHT) as _,
            BORDER_WIDTH as f64 + TITLEBAR_HEIGHT as f64 / 2.0 - extents.y_bearing() / 2.25,
        );

        context.show_text(&title).unwrap();
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn container_id(&self) -> u32 {
        self.container_id
    }

    pub fn x(&self) -> i16 {
        self.x.get()
    }

    pub fn set_x(&self, x: i16) {
        self.x.set(x);

        if !self.maximized() {
            self.app
                .api()
                .set_window_x(self.container_id, self.container_x());
        }
    }

    pub fn y(&self) -> i16 {
        self.y.get()
    }

    pub fn set_y(&self, y: i16) {
        self.y.set(y);

        if !self.maximized() {
            self.app
                .api()
                .set_window_y(self.container_id, self.container_y());
        }
    }

    pub fn width(&self) -> u16 {
        self.width.get()
    }

    pub fn height(&self) -> u16 {
        self.height.get()
    }

    pub fn set_size(&self, width: u16, height: u16) {
        self.width.set(width);
        self.height.set(height);

        if !self.maximized() {
            self.app.api().set_window_width(self.id, self.width());
            self.app.api().set_window_height(self.id, self.height());

            self.app
                .api()
                .set_window_width(self.container_id, self.container_width());

            self.app
                .api()
                .set_window_height(self.container_id, self.container_height());

            self.surface
                .set_size(self.container_width() as _, self.container_height() as _)
                .unwrap();

            self.need_redraw.set(true);
        }
    }

    pub fn maximized(&self) -> bool {
        self.maximized.get()
    }

    pub fn set_maximized(&self, maximized: bool) {
        if maximized == self.maximized() {
            return;
        }

        self.maximized.set(maximized);

        self.app.api().set_window_x(self.id, self.inner_offset_x());
        self.app.api().set_window_y(self.id, self.inner_offset_y());

        self.app
            .api()
            .set_window_x(self.container_id, self.container_x());

        self.app
            .api()
            .set_window_y(self.container_id, self.container_y());

        self.app
            .api()
            .set_window_width(self.container_id, self.container_width());

        self.app
            .api()
            .set_window_height(self.container_id, self.container_height());

        let width = if maximized {
            self.container_width()
        }
        else {
            self.width()
        };

        let height = if maximized {
            self.container_height()
        }
        else {
            self.height()
        };

        self.app.api().set_window_width(self.id, width);
        self.app.api().set_window_height(self.id, height);

        if maximized {
            self.ungrab_buttons_on_container();
        }
        else {
            self.need_redraw.set(true);
            self.grab_buttons_on_container()
        }
    }

    pub fn class(&self) -> Ref<Option<String>> {
        self.class.borrow()
    }

    pub fn set_class(&self, class: Option<String>) {
        *self.class.borrow_mut() = class;
    }

    pub fn title(&self) -> Ref<Option<String>> {
        self.title.borrow()
    }

    pub fn set_title(&self, title: Option<String>) {
        *self.title.borrow_mut() = title;
        self.need_redraw.set(true);
    }

    pub fn icon(&self) -> Ref<Option<cairo::ImageSurface>> {
        self.icon.borrow()
    }

    pub fn set_icon(&self, icon: Option<cairo::ImageSurface>) {
        *self.icon.borrow_mut() = icon;
        self.need_redraw.set(true);
    }

    pub fn notify(&self) {
        self.need_redraw.set(true);
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.app
            .api()
            .reparent_window(self.id, self.app.api().root(), self.x(), self.y());

        self.app.api().remove_from_save_set(self.id);
        self.app.api().destroy_window(self.container_id);
    }
}
