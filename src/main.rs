#![feature(coroutines)]
#![feature(iter_from_coroutine)]
#![feature(slice_as_chunks)]
#![allow(clippy::too_many_arguments)]

mod api;
mod app;
mod bottom_panel;
mod client;
mod keycode;
mod spawner;
mod top_panel;
mod util;
mod wm;

use app::App;
use keycode::get_keys_to_grab;
use nix::libc::STDERR_FILENO;
use nix::libc::STDOUT_FILENO;
use nix::unistd::dup2;
use std::fs::File;
use std::mem::forget;
use std::os::fd::AsRawFd;
use std::time::Duration;
use x11rb::protocol::xproto::EventMask;

fn main() {
    if cfg!(not(debug_assertions)) {
        let file = File::options()
            .create(true)
            .append(true)
            .open("/tmp/vaporwm.log")
            .unwrap();

        redirect_output_to_file(file);
    }

    let app = App::new();

    app.api()
        .set_window_event_mask(
            app.api().root(),
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        )
        .check()
        .expect("There is a window manager running already");

    app.api()
        .set_window_cursor(app.api().root(), app.api().cursors.left_ptr);

    for (keycode, modmask) in get_keys_to_grab() {
        app.api().grab_key(app.api().root(), modmask, keycode);
    }

    loop {
        app.top_panel().request_redraw();
        app.bottom_panel().request_redraw();
        app.wm().request_redraw();
        app.api().flush();

        for event in app.api().wait_for_events(Duration::from_secs(1)) {
            app.wm().handle_event(&event);
            app.top_panel().handle_event(&event);
            app.bottom_panel().handle_event(&event);
            app.spawner().handle_event(&event);
        }
    }
}

fn redirect_output_to_file(file: File) {
    dup2(file.as_raw_fd(), STDOUT_FILENO).unwrap();
    dup2(file.as_raw_fd(), STDERR_FILENO).unwrap();
    forget(file);
}
