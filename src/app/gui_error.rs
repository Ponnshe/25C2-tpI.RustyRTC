use crate::connection_manager::connection_error::ConnectionError;

/// Represents an error in the GUI.
#[derive(Debug)]
pub enum GuiError {
    /// A connection error.
    Connection(ConnectionError),
    /// A rendering error.
    Render,
}

impl From<ConnectionError> for GuiError {
    fn from(e: ConnectionError) -> Self {
        GuiError::Connection(e)
    }
}
