use std::{
    collections::HashMap,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use crate::signaling::auth::{AuthBackend, AuthError, RegisterError};
use crate::signaling::protocol::UserName;

use rand::RngCore;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
struct UserEntry {
    salt: [u8; 16],
    hash: [u8; 32],
}

#[derive(Debug)]
pub struct FileUserStore {
    path: PathBuf,
    users: HashMap<UserName, UserEntry>,
}
impl FileUserStore {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();

        let mut users = HashMap::new();

        if path.exists() {
            let mut file = fs::File::open(&path)?;
            let mut contents = String::new();
            file.read_to_string(&mut contents)?;

            for (line_no, line) in contents.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }

                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() != 3 {
                    eprintln!(
                        "[FileUserStore] ignoring malformed line {} in {:?}: {}",
                        line_no + 1,
                        path,
                        line
                    );
                    continue;
                }

                let username = parts[0].to_string();
                let salt_hex = parts[1];
                let hash_hex = parts[2];

                let salt_vec = match from_hex(salt_hex, 16) {
                    Some(v) => v,
                    None => {
                        eprintln!(
                            "[FileUserStore] bad salt hex on line {}: {}",
                            line_no + 1,
                            salt_hex
                        );
                        continue;
                    }
                };
                let hash_vec = match from_hex(hash_hex, 32) {
                    Some(v) => v,
                    None => {
                        eprintln!(
                            "[FileUserStore] bad hash hex on line {}: {}",
                            line_no + 1,
                            hash_hex
                        );
                        continue;
                    }
                };

                let mut salt = [0u8; 16];
                salt.copy_from_slice(&salt_vec);
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&hash_vec);

                users.insert(username, UserEntry { salt, hash });
            }
        }

        Ok(Self { path, users })
    }

    fn persist(&self) -> io::Result<()> {
        let mut buf = String::new();

        for (username, entry) in &self.users {
            let salt_hex = to_hex(&entry.salt);
            let hash_hex = to_hex(&entry.hash);
            buf.push_str(username);
            buf.push(':');
            buf.push_str(&salt_hex);
            buf.push(':');
            buf.push_str(&hash_hex);
            buf.push('\n');
        }

        // Write to temp file then atomically rename.
        let tmp = self.path.with_extension("tmp");
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(buf.as_bytes())?;
            f.flush()?;
        }
        fs::rename(tmp, &self.path)?;

        Ok(())
    }
}
impl AuthBackend for FileUserStore {
    fn verify(&self, username: &str, password: &str) -> Result<(), AuthError> {
        match self.users.get(username) {
            Some(entry) => {
                let candidate = hash_password(password, &entry.salt);
                if candidate == entry.hash {
                    Ok(())
                } else {
                    Err(AuthError::InvalidCredentials)
                }
            }
            None => Err(AuthError::InvalidCredentials),
        }
    }

    fn register(&mut self, username: &str, password: &str) -> Result<(), RegisterError> {
        // Basic validation: no colons, non-empty.
        if username.is_empty() {
            return Err(RegisterError::InvalidUsername);
        }
        if username.contains(':') {
            return Err(RegisterError::InvalidUsername);
        }
        if password.len() < 6 {
            return Err(RegisterError::WeakPassword);
        }

        if self.users.contains_key(username) {
            return Err(RegisterError::UsernameTaken);
        }

        // Generate random salt (16 bytes).
        let mut salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut salt);

        let hash = hash_password(password, &salt);

        self.users
            .insert(username.to_owned(), UserEntry { salt, hash });

        // Persist to disk; if it fails, roll back and signal Internal.
        if let Err(_) = self.persist() {
            self.users.remove(username);
            return Err(RegisterError::Internal);
        }

        Ok(())
    }
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

// Parse exactly `expected_len` bytes from hex; return None if format is wrong.
fn from_hex(input: &str, expected_len: usize) -> Option<Vec<u8>> {
    if input.len() != expected_len * 2 {
        return None;
    }
    let mut out = Vec::with_capacity(expected_len);
    let chars: Vec<_> = input.as_bytes().to_vec();
    for i in (0..chars.len()).step_by(2) {
        let hi = chars[i];
        let lo = chars[i + 1];
        let v = (hex_val(hi)? << 4) | hex_val(lo)?;
        out.push(v);
    }
    Some(out)
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(10 + c - b'a'),
        b'A'..=b'F' => Some(10 + c - b'A'),
        _ => None,
    }
}

fn hash_password(password: &str, salt: &[u8; 16]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(password.as_bytes());
    let result = hasher.finalize(); // 32 bytes
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn unique_temp_path() -> PathBuf {
        // Use RngCore since it's already in scope in this module.
        let mut bytes = [0u8; 8];
        rand::thread_rng().fill_bytes(&mut bytes);
        let suffix = u64::from_le_bytes(bytes);
        std::env::temp_dir().join(format!("file_user_store_test_{suffix}.db"))
    }

    #[test]
    fn register_and_verify_roundtrip_persists_to_disk() {
        let path = unique_temp_path();
        // Just in case; ignore error if file doesn't exist.
        let _ = fs::remove_file(&path);

        // 1) Open store (on a non-existent file, should start empty)
        {
            let mut store = FileUserStore::open(&path).expect("open FileUserStore");
            assert!(
                store.users.is_empty(),
                "new store for non-existing file should be empty"
            );

            // Register a user
            let res = store.register("alice", "supersecret");
            assert!(
                res.is_ok(),
                "registration should succeed for fresh username: {res:?}"
            );

            // Verify correct password
            let ok = store.verify("alice", "supersecret");
            assert!(ok.is_ok(), "verify should succeed with correct password");

            // Wrong password should fail
            let bad = store.verify("alice", "wrongpw");
            match bad {
                Err(AuthError::InvalidCredentials) => {}
                other => panic!("expected InvalidCredentials, got {other:?}"),
            }
        }

        // 2) Reopen from disk and verify again
        {
            let store = FileUserStore::open(&path).expect("reopen FileUserStore");

            let ok = store.verify("alice", "supersecret");
            assert!(
                ok.is_ok(),
                "verify should still succeed after reopen with same password"
            );

            let bad = store.verify("alice", "wrongpw");
            match bad {
                Err(AuthError::InvalidCredentials) => {}
                other => panic!("expected InvalidCredentials after reopen, got {other:?}"),
            }
        }

        // Cleanup (best-effort)
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn duplicate_username_is_rejected() {
        let path = unique_temp_path();
        let _ = fs::remove_file(&path);

        let mut store = FileUserStore::open(&path).expect("open FileUserStore");

        assert!(store.register("bob", "password1").is_ok());

        let dup = store.register("bob", "anotherpw");
        match dup {
            Err(RegisterError::UsernameTaken) => {}
            other => panic!("expected UsernameTaken, got {other:?}"),
        }

        let ok = store.verify("bob", "password1");
        assert!(ok.is_ok(), "original password should still work");
    }

    #[test]
    fn invalid_username_and_weak_password_are_rejected() {
        let path = unique_temp_path();
        let _ = fs::remove_file(&path);

        let mut store = FileUserStore::open(&path).expect("open FileUserStore");

        // Empty username
        match store.register("", "somepw") {
            Err(RegisterError::InvalidUsername) => {}
            other => panic!("expected InvalidUsername for empty username, got {other:?}"),
        }

        // Username with colon
        match store.register("bad:name", "somepw") {
            Err(RegisterError::InvalidUsername) => {}
            other => panic!("expected InvalidUsername for username with colon, got {other:?}"),
        }

        // Weak password (len < 6)
        match store.register("charlie", "123") {
            Err(RegisterError::WeakPassword) => {}
            other => panic!("expected WeakPassword for short password, got {other:?}"),
        }
    }
}
