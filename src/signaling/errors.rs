use crate::signaling::auth::RegisterError;

#[repr(u16)]
#[derive(Debug, Copy, Clone)]
pub enum LoginErrorCode {
    AlreadyLoggedIn = 1,
    NotAuthorized = 2,
    InvalidCredentials = 3,
    Internal = 4,
}

impl LoginErrorCode {
    pub fn as_u16(self) -> u16 {
        self as u16
    }
}

#[repr(u16)]
#[derive(Debug, Copy, Clone)]
pub enum JoinErrorCode {
    NotLoggedIn = 10,
    NotFound = 20,
    Full = 21,
}

impl JoinErrorCode {
    pub fn as_u16(self) -> u16 {
        self as u16
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterErrorCode {
    UsernameTaken = 1,
    InvalidUsername = 2,
    WeakPassword = 3,
    Internal = 4,
    Unsupported = 5,
}

impl RegisterErrorCode {
    pub fn as_u16(self) -> u16 {
        self as u16
    }
}

impl From<RegisterError> for RegisterErrorCode {
    fn from(err: RegisterError) -> Self {
        match err {
            RegisterError::UsernameTaken => RegisterErrorCode::UsernameTaken,
            RegisterError::InvalidUsername => RegisterErrorCode::InvalidUsername,
            RegisterError::WeakPassword => RegisterErrorCode::WeakPassword,
            RegisterError::Internal => RegisterErrorCode::Internal,
            RegisterError::Unsupported => RegisterErrorCode::Unsupported,
        }
    }
}
