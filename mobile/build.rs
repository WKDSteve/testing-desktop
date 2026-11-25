//! The build script of the mobile shell: it runs the Tauri build step that generates the context of the app.

fn main() {
    tauri_build::build();
}
