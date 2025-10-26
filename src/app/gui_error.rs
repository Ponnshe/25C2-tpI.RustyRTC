use crate::connection_manager::connection_error::ConnectionError;

#[derive(Debug)]
pub enum GuiError {
    Connection(ConnectionError),
    Render,
}

impl From<ConnectionError> for GuiError {
    fn from(e: ConnectionError) -> Self {
        GuiError::Connection(e)
    }
}
