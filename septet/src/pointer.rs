//! Where the pointer is when files are dropped from the file manager. winit 0.30's file drops carry
//! no position, and during an OS drag a window gets no pointer events, so egui's last pointer is
//! stale. On X11 we ask the server, on Windows the system; on macOS the apps keep their own drop
//! handling.

use egui::Pos2;

/// The pointer in the current viewport's coordinates (points), if the platform can tell.
pub fn in_viewport(ctx: &egui::Context) -> Option<Pos2> {
    let inner = ctx.input(|i| i.viewport().inner_rect)?;
    let ppp = ctx.pixels_per_point();
    let (x, y) = platform::root_pointer()?;
    Some(Pos2::new(x / ppp - inner.min.x, y / ppp - inner.min.y))
}

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use std::sync::Mutex;

    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::ConnectionExt;
    use x11rb::rust_connection::RustConnection;

    /// One connection, opened on first use.
    static CONN: Mutex<Option<(RustConnection, u32)>> = Mutex::new(None);

    /// The pointer on the root window, in pixels.
    pub fn root_pointer() -> Option<(f32, f32)> {
        std::env::var_os("DISPLAY")?;
        let mut guard = CONN.lock().ok()?;
        if guard.is_none() {
            let (conn, screen) = RustConnection::connect(None).ok()?;
            let root = conn.setup().roots.get(screen)?.root;
            *guard = Some((conn, root));
        }
        let (conn, root) = guard.as_ref()?;
        match conn.query_pointer(*root).ok().and_then(|c| c.reply().ok()) {
            Some(r) => Some((f32::from(r.root_x), f32::from(r.root_y))),
            None => {
                *guard = None;
                None
            }
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    /// The pointer on the virtual screen, in pixels (winit makes the process per-monitor DPI aware).
    pub fn root_pointer() -> Option<(f32, f32)> {
        let p = winsafe::GetCursorPos().ok()?;
        Some((p.x as f32, p.y as f32))
    }
}

#[cfg(not(any(target_os = "windows", all(unix, not(target_os = "macos")))))]
mod platform {
    pub fn root_pointer() -> Option<(f32, f32)> {
        None
    }
}
