use crate::keycode::Keycode;
use std::process::Command;
use std::process::Stdio;
use x11rb::protocol::Event;

pub struct Spawner;

impl Spawner {
    pub fn new() -> Self {
        Self
    }

    pub fn handle_event(&self, event: &Event) {
        if let Event::KeyPress(event) = event {
            let Ok(keycode) = Keycode::try_from(event.detail)
            else {
                return;
            };

            match keycode {
                Keycode::PrintScreen => bash("maim --hidecursor | xclip -selection clipboard -t image/png"),
                Keycode::S => bash("maim --select --highlight --color=255,255,255,0.05 --hidecursor | xclip -selection clipboard -t image/png"),
                Keycode::T => bash("xfce4-terminal &"),
                Keycode::D => bash("thunar &"),
                Keycode::G => bash("xfce4-taskmanager &"),
                Keycode::B => bash("firefox &"),
                Keycode::Q => bash("copyq show &"),
                Keycode::R => bash("rofi -show drun &"),
                _ => {}
            }
        }
    }
}

fn bash(command: &str) {
    Command::new("bash")
        .args(["-c", command])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
        .wait()
        .unwrap();
}
