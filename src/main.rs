mod app;
mod ipc;
mod loader;
mod shell;
mod surface;
mod theme;
mod util;
mod views;

fn main() -> Result<(), iced_layershell::Error> {
    app::run()
}
