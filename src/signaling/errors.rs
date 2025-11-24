#[repr(u16)]
#[derive(Debug, Copy, Clone)]
pub enum LoginErrorCode {
    AlreadyLoggedIn = 1,
    NotAuthorized = 2,
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
