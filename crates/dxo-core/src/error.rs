use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("HID error: {0}")]
    Hid(#[from] hidapi::HidError),
    #[error("no supported controller found")]
    NoDevice,
    #[error("report too short: got {got} bytes, need {need}")]
    ShortReport { got: usize, need: usize },
    #[error("unexpected report id 0x{0:02X}")]
    BadReportId(u8),
    #[error("virtual pad error: {0}")]
    Virtual(String),
    #[error("HidHide error: {0}")]
    HidHide(String),
    #[error("profile error: {0}")]
    Profile(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not supported on this platform/milestone: {0}")]
    Unsupported(&'static str),
}

pub type Result<T> = std::result::Result<T, Error>;
