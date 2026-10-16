mod app;
mod domain;
mod errors;
mod ffmpeg;
mod merge;
mod xdm;

fn main() -> iced::Result {
    app::run()
}
