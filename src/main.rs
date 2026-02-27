mod app;
mod demo;
mod events;
mod ipc;
mod loader;
mod session;
mod shell;
mod surface;
mod theme;
mod util;
mod views;
mod watcher;

fn main() -> Result<(), iced_layershell::Error> {
    app::run()
}
