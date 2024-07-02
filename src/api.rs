use crate::keycode::Keycode;
use nix::poll::poll;
use nix::poll::PollFd;
use nix::poll::PollFlags;
use std::os::fd::AsRawFd;
use std::os::fd::BorrowedFd;
use std::time::Duration;
use x11rb::atom_manager;
use x11rb::connection::Connection;
use x11rb::cookie::VoidCookie;
use x11rb::properties::WmClassCookie;
use x11rb::protocol::xproto::Allow;
use x11rb::protocol::xproto::AtomEnum;
use x11rb::protocol::xproto::ButtonIndex;
use x11rb::protocol::xproto::ChangeWindowAttributesAux;
use x11rb::protocol::xproto::ClientMessageData;
use x11rb::protocol::xproto::ClientMessageEvent;
use x11rb::protocol::xproto::ColormapAlloc;
use x11rb::protocol::xproto::ConfigWindow;
use x11rb::protocol::xproto::ConfigureRequestEvent;
use x11rb::protocol::xproto::ConfigureWindowAux;
use x11rb::protocol::xproto::ConnectionExt;
use x11rb::protocol::xproto::CreateWindowAux;
use x11rb::protocol::xproto::EventMask;
use x11rb::protocol::xproto::GetGeometryReply;
use x11rb::protocol::xproto::GetWindowAttributesReply;
use x11rb::protocol::xproto::GrabMode;
use x11rb::protocol::xproto::InputFocus;
use x11rb::protocol::xproto::ModMask;
use x11rb::protocol::xproto::PropMode;
use x11rb::protocol::xproto::Screen;
use x11rb::protocol::xproto::SetMode;
use x11rb::protocol::xproto::StackMode;
use x11rb::protocol::xproto::VisualClass;
use x11rb::protocol::xproto::Visualtype;
use x11rb::protocol::xproto::WindowClass;
use x11rb::protocol::Event;
use x11rb::resource_manager;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::xcb_ffi::XCBConnection;

pub const ICON_SIZE: u16 = 16;

macro_rules! define_cursors {
    (
        $struct_vis:vis $struct_name:ident($cookie_vis:vis $cookie_name:ident) {
            $($cursor:ident,)*
        }
    ) => {
        $struct_vis struct $struct_name {
            $(
                pub $cursor: u32,
            )*
        }

        $cookie_vis struct $cookie_name<'a, 'b, C: x11rb::connection::Connection> {
            connection: &'a C,

            $(
                pub $cursor: x11rb::cursor::Cookie<'a, 'b, C>,
            )*
        }

        impl<'a, 'b, C: x11rb::connection::Connection> $cookie_name<'a, 'b, C> {
            pub fn new(connection: &'a C, db: &'b x11rb::resource_manager::Database, screen_num: usize) -> Self {
                Self {
                    connection,

                    $(
                        $cursor: x11rb::cursor::Handle::new(connection, screen_num, db).unwrap(),
                    )*
                }
            }

            pub fn reply(self) -> $struct_name {
                $struct_name {
                    $(
                        $cursor: self
                            .$cursor
                            .reply()
                            .unwrap()
                            .load_cursor(self.connection, stringify!($cursor))
                            .unwrap(),
                    )*
                }
            }
        }
    };
}

define_cursors! {
    pub Cursors(CursorsCookie) {
        fleur,
        bottom_right_corner,
        left_ptr,
        hand,
    }
}

atom_manager! {
    pub Atoms:

    AtomsCookie {
        WM_PROTOCOLS,
        WM_DELETE_WINDOW,
        WM_STATE,
        _NET_WM_NAME,
        _NET_WM_ICON,
        UTF8_STRING,
    }
}

pub struct Api {
    connection: XCBConnection,
    screen_index: usize,
    pub cursors: Cursors,
    pub atoms: Atoms,
    visual_id: u32,
    colormap_id: u32,
    cairo: Cairo,
    pub default_icon: cairo::ImageSurface,
}

impl Api {
    pub fn new() -> Self {
        let (connection, screen_index) = XCBConnection::connect(None).unwrap();
        let screen = &connection.setup().roots[screen_index];

        let visual = screen
            .allowed_depths
            .iter()
            .find(|depth| depth.depth == 32)
            .unwrap()
            .visuals
            .iter()
            .find(|visual| visual.class == VisualClass::TRUE_COLOR)
            .unwrap();

        let visual_id = visual.visual_id;
        let colormap_id = create_colormap(&connection, screen, visual_id);
        let cairo = Cairo::new(&connection, visual);

        let db = resource_manager::new_from_default(&connection).unwrap();
        let cursors = CursorsCookie::new(&connection, &db, screen_index).reply();
        let atoms = Atoms::new(&connection).unwrap().reply().unwrap();

        Self {
            connection,
            screen_index,
            cursors,
            atoms,
            visual_id,
            colormap_id,
            cairo,
            default_icon: {
                let mut stream = include_bytes!("../assets/default-icon.png").as_slice();
                cairo::ImageSurface::create_from_png(&mut stream).unwrap()
            },
        }
    }

    fn screen(&self) -> &Screen {
        &self.connection.setup().roots[self.screen_index]
    }

    pub fn root(&self) -> u32 {
        self.screen().root
    }

    pub fn screen_width(&self) -> u16 {
        self.screen().width_in_pixels
    }

    pub fn screen_height(&self) -> u16 {
        self.screen().height_in_pixels
    }

    pub fn put_wm_state_property(&self, window: u32) {
        check(
            self.connection
                .change_property32(
                    PropMode::REPLACE,
                    window,
                    self.atoms.WM_STATE,
                    self.atoms.WM_STATE,
                    &[1, x11rb::NONE],
                )
                .unwrap(),
        );
    }

    pub fn set_window_x(&self, window: u32, x: i16) {
        check(
            self.connection
                .configure_window(window, &ConfigureWindowAux::new().x(x as i32))
                .unwrap(),
        );
    }

    pub fn set_window_y(&self, window: u32, y: i16) {
        check(
            self.connection
                .configure_window(window, &ConfigureWindowAux::new().y(y as i32))
                .unwrap(),
        );
    }

    pub fn set_window_width(&self, window: u32, width: u16) {
        check(
            self.connection
                .configure_window(window, &ConfigureWindowAux::new().width(width as u32))
                .unwrap(),
        );
    }

    pub fn set_window_height(&self, window: u32, height: u16) {
        check(
            self.connection
                .configure_window(window, &ConfigureWindowAux::new().height(height as u32))
                .unwrap(),
        );
    }

    pub fn set_window_border_width(&self, window: u32, border_width: u16) {
        check(
            self.connection
                .configure_window(
                    window,
                    &ConfigureWindowAux::new().border_width(border_width as u32),
                )
                .unwrap(),
        );
    }

    pub fn set_window_event_mask(
        &self,
        window: u32,
        event_mask: EventMask,
    ) -> VoidCookie<'_, XCBConnection> {
        self.connection
            .change_window_attributes(
                window,
                &ChangeWindowAttributesAux::new().event_mask(event_mask),
            )
            .unwrap()
    }

    pub fn get_window_geometry(&self, window: u32) -> GetGeometryReply {
        self.connection
            .get_geometry(window)
            .unwrap()
            .reply()
            .unwrap()
    }

    pub fn get_window_class(&self, window: u32) -> Option<String> {
        WmClassCookie::new(&self.connection, window)
            .unwrap()
            .reply()
            .ok()
            .map(|reply| String::from_utf8_lossy(reply.class()).into_owned())
    }

    pub fn get_window_title(&self, window: u32) -> Option<String> {
        let reply = self
            .connection
            .get_property(
                false,
                window,
                self.atoms._NET_WM_NAME,
                self.atoms.UTF8_STRING,
                0,
                u32::MAX,
            )
            .unwrap()
            .reply()
            .unwrap();

        (reply.type_ == self.atoms.UTF8_STRING)
            .then(|| String::from_utf8_lossy(&reply.value).into_owned())
    }

    pub fn set_window_cursor(&self, window: u32, cursor: u32) {
        check(
            self.connection
                .change_window_attributes(window, &ChangeWindowAttributesAux::new().cursor(cursor))
                .unwrap(),
        );
    }

    pub fn create_window(
        &self,
        window: u32,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        values: CreateWindowAux,
    ) {
        check(
            self.connection
                .create_window(
                    32,
                    window,
                    self.root(),
                    x,
                    y,
                    width,
                    height,
                    0,
                    WindowClass::INPUT_OUTPUT,
                    self.visual_id,
                    &values.colormap(self.colormap_id).border_pixel(0),
                )
                .unwrap(),
        );
    }

    pub fn grab_key(&self, window: u32, modmask: ModMask, keycode: Keycode) {
        check(
            self.connection
                .grab_key(
                    false,
                    window,
                    modmask,
                    keycode as u8,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                )
                .unwrap(),
        );
    }

    pub fn flush(&self) {
        self.connection.flush().unwrap();
    }

    pub fn wait_for_events(&self, duration: Duration) -> impl Iterator<Item = Event> + '_ {
        // SAFETY: connection definitely lives long enough
        let fd = unsafe { BorrowedFd::borrow_raw(self.connection.as_raw_fd()) };
        let fds = &mut [PollFd::new(&fd, PollFlags::POLLIN)];
        poll(fds, duration.as_millis() as _).unwrap();

        std::iter::from_coroutine(|| {
            while let Some(event) = self.connection.poll_for_event().unwrap() {
                yield event;
            }
        })
    }

    pub fn generate_id(&self) -> u32 {
        self.connection.generate_id().unwrap()
    }

    pub fn map_window(&self, window: u32) {
        check(self.connection.map_window(window).unwrap());
    }

    pub fn unmap_window(&self, window: u32) {
        check(self.connection.unmap_window(window).unwrap());
    }

    pub fn create_cairo_xcb_surface(
        &self,
        window: u32,
        width: u16,
        height: u16,
    ) -> cairo::XCBSurface {
        cairo::XCBSurface::create(
            &self.cairo.connection,
            &cairo::XCBDrawable(window),
            &self.cairo.visual,
            width as _,
            height as _,
        )
        .unwrap()
    }

    pub fn reparent_window(&self, window: u32, parent: u32, offset_x: i16, offset_y: i16) {
        check(
            self.connection
                .reparent_window(window, parent, offset_x, offset_y)
                .unwrap(),
        );
    }

    pub fn add_to_save_set(&self, window: u32) {
        check(
            self.connection
                .change_save_set(SetMode::INSERT, window)
                .unwrap(),
        );
    }

    pub fn remove_from_save_set(&self, window: u32) {
        check(
            self.connection
                .change_save_set(SetMode::DELETE, window)
                .unwrap(),
        );
    }

    pub fn grab_button(
        &self,
        window: u32,
        event_mask: EventMask,
        button: ButtonIndex,
        modmask: ModMask,
        cursor: u32,
        owner_events: bool,
        pointer_mode: GrabMode,
        keyboard_mode: GrabMode,
        confine_to: u32,
    ) {
        check(
            self.connection
                .grab_button(
                    owner_events,
                    window,
                    event_mask,
                    pointer_mode,
                    keyboard_mode,
                    confine_to,
                    cursor,
                    button,
                    modmask,
                )
                .unwrap(),
        );
    }

    pub fn ungrab_button(&self, window: u32, button: ButtonIndex, modmask: ModMask) {
        check(
            self.connection
                .ungrab_button(button, window, modmask)
                .unwrap(),
        );
    }

    pub fn destroy_window(&self, window: u32) {
        check(self.connection.destroy_window(window).unwrap());
    }

    pub fn ask_window_to_close(&self, window: u32) {
        check(
            self.connection
                .send_event(
                    false,
                    window,
                    EventMask::NO_EVENT,
                    ClientMessageEvent {
                        response_type: 33,
                        format: 32,
                        sequence: 0,
                        window,
                        type_: self.atoms.WM_PROTOCOLS,
                        data: ClientMessageData::from([
                            self.atoms.WM_DELETE_WINDOW,
                            x11rb::CURRENT_TIME,
                            0,
                            0,
                            0,
                        ]),
                    },
                )
                .unwrap(),
        );
    }

    pub fn allow_pointer_events(&self) {
        check(
            self.connection
                .allow_events(Allow::REPLAY_POINTER, x11rb::CURRENT_TIME)
                .unwrap(),
        );
    }

    pub fn move_pointer(&self, x: u16, y: u16) {
        check(
            self.connection
                .warp_pointer(x11rb::NONE, self.root(), 0, 0, 0, 0, x as _, y as _)
                .unwrap(),
        );
    }

    pub fn set_focus(&self, window: impl Into<Option<u32>>) {
        check(
            self.connection
                .set_input_focus(
                    InputFocus::NONE,
                    window.into().unwrap_or(self.root()),
                    x11rb::CURRENT_TIME,
                )
                .unwrap(),
        );
    }

    pub fn raise_window(&self, window: u32) {
        check(
            self.connection
                .configure_window(
                    window,
                    &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
                )
                .unwrap(),
        );
    }

    pub fn get_window_children(&self, window: u32) -> Vec<u32> {
        self.connection
            .query_tree(window)
            .unwrap()
            .reply()
            .unwrap()
            .children
    }

    pub fn get_window_attributes(&self, window: u32) -> GetWindowAttributesReply {
        self.connection
            .get_window_attributes(window)
            .unwrap()
            .reply()
            .unwrap()
    }

    pub fn get_window_icon(&self, window: u32) -> Option<cairo::ImageSurface> {
        let reply = self
            .connection
            .get_property(
                false,
                window,
                self.atoms._NET_WM_ICON,
                AtomEnum::CARDINAL,
                0,
                u32::MAX,
            )
            .unwrap()
            .reply()
            .unwrap();

        if reply.value.is_empty() {
            return None;
        }

        let mut buffer = reply.value.as_slice();
        let mut icons = Vec::new();

        loop {
            let width = u32::from_ne_bytes(buffer.get(..4)?.try_into().unwrap());
            let height = u32::from_ne_bytes(buffer.get(4..8)?.try_into().unwrap());
            let length = width as usize * height as usize * 4;
            let data = buffer.get(8..(8 + length))?;

            icons.push(Icon {
                width,
                height,
                data,
            });

            buffer = match buffer.get(8 + length..) {
                Some(buffer) => buffer,
                None => break,
            };

            if buffer.is_empty() {
                break;
            }
        }

        let icon = find_most_appropriate_icon(&icons)?;
        let image = icon.to_image()?;

        if !(icon.width == ICON_SIZE as u32 && icon.height == ICON_SIZE as u32) {
            let size = icon.width.max(icon.height);
            let ratio = size as f64 / ICON_SIZE as f64;
            image.set_device_scale(ratio, ratio);

            let new_image =
                cairo::ImageSurface::create(cairo::Format::ARgb32, ICON_SIZE as _, ICON_SIZE as _)
                    .unwrap();

            let context = cairo::Context::new(&new_image).unwrap();

            context.set_source_surface(&image, 0.0, 0.0).unwrap();
            context.source().set_filter(cairo::Filter::Nearest);
            context.paint().unwrap();

            return Some(new_image);
        }

        Some(image)
    }

    pub fn allow_configure_request(&self, event: &ConfigureRequestEvent) {
        check(
            self.connection
                .configure_window(
                    event.window,
                    &ConfigureWindowAux {
                        x: event
                            .value_mask
                            .contains(ConfigWindow::X)
                            .then_some(event.x as _),
                        y: event
                            .value_mask
                            .contains(ConfigWindow::Y)
                            .then_some(event.y as _),
                        width: event
                            .value_mask
                            .contains(ConfigWindow::WIDTH)
                            .then_some(event.width as _),
                        height: event
                            .value_mask
                            .contains(ConfigWindow::HEIGHT)
                            .then_some(event.height as _),
                        border_width: event
                            .value_mask
                            .contains(ConfigWindow::BORDER_WIDTH)
                            .then_some(event.border_width as _),
                        sibling: event
                            .value_mask
                            .contains(ConfigWindow::SIBLING)
                            .then_some(event.sibling as _),
                        stack_mode: event
                            .value_mask
                            .contains(ConfigWindow::STACK_MODE)
                            .then_some(event.stack_mode as _),
                    },
                )
                .unwrap(),
        );
    }
}

fn create_colormap(connection: &XCBConnection, screen: &Screen, visual_id: u32) -> u32 {
    let colormap_id = connection.generate_id().unwrap();

    connection
        .create_colormap(ColormapAlloc::NONE, colormap_id, screen.root, visual_id)
        .unwrap();

    colormap_id
}

struct Cairo {
    connection: cairo::XCBConnection,
    visual: cairo::XCBVisualType,
    _visual: Box<XCBVisualType>,
}

impl Cairo {
    fn new(connection: &XCBConnection, visual: &Visualtype) -> Self {
        let mut xcb_visual_type = Box::new(XCBVisualType {
            visual_id: visual.visual_id,
            class: visual.class.into(),
            bits_per_rgb_value: visual.bits_per_rgb_value,
            colormap_entries: visual.colormap_entries,
            red_mask: visual.red_mask,
            green_mask: visual.green_mask,
            blue_mask: visual.blue_mask,
            pad0: [0; 4],
        });

        unsafe {
            // SAFETY: connection and cairo_connection will have the same lifetime
            let cairo_connection =
                cairo::XCBConnection::from_raw_none(connection.get_raw_xcb_connection() as _);

            // SAFETY: xcb_visual_type and cairo_visual will have the same lifetime
            let cairo_visual =
                cairo::XCBVisualType::from_raw_none(xcb_visual_type.as_mut() as *mut _ as _);

            Self {
                connection: cairo_connection,
                visual: cairo_visual,
                _visual: xcb_visual_type,
            }
        }
    }
}

#[repr(C)]
struct XCBVisualType {
    visual_id: u32,
    class: u8,
    bits_per_rgb_value: u8,
    colormap_entries: u16,
    red_mask: u32,
    green_mask: u32,
    blue_mask: u32,
    pad0: [u8; 4],
}

#[cfg(debug_assertions)]
fn check(request: VoidCookie<'_, XCBConnection>) {
    request.check().unwrap();
}

#[cfg(not(debug_assertions))]
fn check(_request: VoidCookie<'_, XCBConnection>) {}

struct Icon<'a> {
    width: u32,
    height: u32,
    data: &'a [u8],
}

impl<'a> Icon<'a> {
    fn is_better_than(&self, other: &Icon) -> bool {
        let self_delta_width = ICON_SIZE as i32 - self.width as i32;
        let self_delta_height = ICON_SIZE as i32 - self.height as i32;

        let other_delta_width = ICON_SIZE as i32 - other.width as i32;
        let other_delta_height = ICON_SIZE as i32 - other.height as i32;

        let better_by_width = self_delta_width < other_delta_width;
        let better_by_height = self_delta_height < other_delta_height;
        let is_square = self.width == self.height;

        let totally_better = better_by_width && better_by_height;
        let somewhat_better = better_by_width || better_by_height;

        totally_better || (somewhat_better && is_square)
    }

    fn to_image(&self) -> Option<cairo::ImageSurface> {
        let (chunks, remainder) = self.data.as_chunks::<4>();

        if !remainder.is_empty() {
            return None;
        }

        let buffer = chunks
            .iter()
            .flat_map(|[b, g, r, a]| {
                [
                    (((*b as u16) * (*a as u16)) / 255) as u8,
                    (((*g as u16) * (*a as u16)) / 255) as u8,
                    (((*r as u16) * (*a as u16)) / 255) as u8,
                    *a,
                ]
            })
            .collect::<Vec<_>>();

        cairo::ImageSurface::create_for_data(
            buffer,
            cairo::Format::ARgb32,
            self.width as _,
            self.height as _,
            (self.width * 4) as _,
        )
        .ok()
    }
}

fn find_most_appropriate_icon<'a, 'b>(icons: &'a [Icon<'b>]) -> Option<&'a Icon<'b>> {
    let mut result = icons.first()?;

    if result.width == ICON_SIZE as u32 && result.height == ICON_SIZE as u32 {
        return Some(result);
    }

    for icon in icons.iter().skip(1) {
        if icon.width == ICON_SIZE as u32 && icon.height == ICON_SIZE as u32 {
            return Some(icon);
        }

        if icon.is_better_than(result) {
            result = icon;
        }
    }

    Some(result)
}
