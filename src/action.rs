//! What the interface asks for. Views only emit these; `App` applies them
//! after the frame is drawn.

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    SignIn,
    SignOut,
    PlayLink(String),
    Play,
    Pause,
    Stop,
    Seek(u32),
    /// 0–100, as the slider shows it.
    Volume(u8),
}
