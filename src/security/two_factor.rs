//! Configurable two-factor authentication for Telegram bot access.
//!
//! Supports four methods:
//! - None: no 2FA (default)
//! - TOTP: time-based one-time passwords (Google Authenticator compatible)
//! - Password: argon2-hashed password challenge
//! - Weblink: one-time UUID token confirmed via web dashboard

use std::collections::HashMap;
use std::sync::Mutex;

/// Two-factor authentication method.
#[derive(Debug, Clone)]
pub enum TwoFactorMethod {
    /// No 2FA required.
    None,
    /// TOTP (Google Authenticator, Authy, etc.).
    Totp { secret: String },
    /// Password challenge (argon2 hash).
    Password { hash: String },
    /// Web dashboard confirmation link.
    Weblink,
}

/// An authenticated session for a chat ID.
#[derive(Debug, Clone)]
struct Session {
    authenticated_at: u64,
}

/// A pending weblink confirmation token.
#[derive(Debug, Clone)]
struct PendingWeblink {
    chat_id: i64,
    created_at: u64,
}

/// Weblink token expiry: 5 minutes.
const WEBLINK_EXPIRY_SECS: u64 = 300;

/// Default session TTL: 12 hours.
const DEFAULT_SESSION_TTL_SECS: u64 = 12 * 60 * 60;

/// Two-factor authentication manager.
pub struct TwoFactorAuth {
    method: TwoFactorMethod,
    sessions: Mutex<HashMap<i64, Session>>,
    pending_weblinks: Mutex<HashMap<String, PendingWeblink>>,
    /// Session time-to-live in seconds. Default: 12 hours.
    pub session_ttl_secs: u64,
}

impl std::fmt::Debug for TwoFactorAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwoFactorAuth")
            .field("method", &self.method)
            .field("session_ttl_secs", &self.session_ttl_secs)
            .finish()
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn lock_mutex<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

impl TwoFactorAuth {
    /// Create a new TwoFactorAuth with the given method.
    pub fn new(method: TwoFactorMethod) -> Self {
        Self {
            method,
            sessions: Mutex::new(HashMap::new()),
            pending_weblinks: Mutex::new(HashMap::new()),
            session_ttl_secs: DEFAULT_SESSION_TTL_SECS,
        }
    }

    /// Create from environment variables.
    ///
    /// - `NABA_TELEGRAM_2FA`: "none", "totp", "password", "weblink"
    /// - `NABA_TOTP_SECRET`: base32-encoded TOTP secret (required if method=totp)
    /// - `NABA_2FA_PASSWORD_HASH`: argon2 password hash (required if method=password)
    pub fn from_env() -> Self {
        let method_str = std::env::var("NABA_TELEGRAM_2FA").unwrap_or_else(|_| "none".to_string());

        let method = match method_str.to_lowercase().as_str() {
            "totp" => {
                let secret = std::env::var("NABA_TOTP_SECRET")
                    .expect("NABA_TOTP_SECRET must be set when NABA_TELEGRAM_2FA=totp");
                TwoFactorMethod::Totp { secret }
            }
            "password" => {
                let hash = std::env::var("NABA_2FA_PASSWORD_HASH")
                    .expect("NABA_2FA_PASSWORD_HASH must be set when NABA_TELEGRAM_2FA=password");
                TwoFactorMethod::Password { hash }
            }
            "weblink" => TwoFactorMethod::Weblink,
            _ => TwoFactorMethod::None,
        };

        Self::new(method)
    }

    /// Check if a chat ID has a valid (non-expired) session.
    pub fn is_authenticated(&self, chat_id: i64) -> bool {
        match self.method {
            TwoFactorMethod::None => true,
            _ => {
                let sessions = lock_mutex(&self.sessions);
                if let Some(session) = sessions.get(&chat_id) {
                    let elapsed = now_secs().saturating_sub(session.authenticated_at);
                    elapsed < self.session_ttl_secs
                } else {
                    false
                }
            }
        }
    }

    /// Returns true if the configured method requires a challenge (i.e., not None).
    pub fn requires_challenge(&self) -> bool {
        !matches!(self.method, TwoFactorMethod::None)
    }

    /// Returns the challenge prompt for the current method.
    pub fn challenge_prompt(&self) -> String {
        match &self.method {
            TwoFactorMethod::None => String::new(),
            TwoFactorMethod::Totp { .. } => {
                "Two-factor authentication required.\nPlease enter your 6-digit TOTP code.".to_string()
            }
            TwoFactorMethod::Password { .. } => {
                "Two-factor authentication required.\nPlease enter your password.".to_string()
            }
            TwoFactorMethod::Weblink => {
                "Two-factor authentication required.\nA confirmation link has been generated. Please confirm via the web dashboard.".to_string()
            }
        }
    }

    /// Verify a TOTP code for the given chat ID. On success, creates a session.
    pub fn verify_totp(&self, chat_id: i64, code: &str) -> bool {
        if let TwoFactorMethod::Totp { ref secret } = self.method {
            let secret_bytes = match totp_rs::Secret::Encoded(secret.clone()).to_bytes() {
                Ok(b) => b,
                Err(_) => return false,
            };
            let totp = match totp_rs::TOTP::new(
                totp_rs::Algorithm::SHA1,
                6,
                1,
                30,
                secret_bytes,
                None,
                "NyayaAgent".to_string(),
            ) {
                Ok(t) => t,
                Err(_) => return false,
            };
            if totp.check_current(code).unwrap_or(false) {
                self.create_session(chat_id);
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Verify a password for the given chat ID. On success, creates a session.
    pub fn verify_password(&self, chat_id: i64, password: &str) -> bool {
        if let TwoFactorMethod::Password { ref hash } = self.method {
            use argon2::PasswordVerifier;
            let parsed_hash = match argon2::PasswordHash::new(hash) {
                Ok(h) => h,
                Err(_) => return false,
            };
            let argon2 = argon2::Argon2::default();
            if argon2
                .verify_password(password.as_bytes(), &parsed_hash)
                .is_ok()
            {
                self.create_session(chat_id);
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Generate a weblink token for a chat ID. Returns the UUID token string.
    pub fn generate_weblink_token(&self, chat_id: i64) -> String {
        let token = uuid::Uuid::new_v4().to_string();
        let mut pending = lock_mutex(&self.pending_weblinks);
        pending.insert(
            token.clone(),
            PendingWeblink {
                chat_id,
                created_at: now_secs(),
            },
        );
        token
    }

    /// Confirm a weblink token. On success, creates a session for the associated chat ID.
    /// Returns true if the token was valid and not expired.
    pub fn confirm_weblink(&self, token: &str) -> bool {
        let mut pending = lock_mutex(&self.pending_weblinks);
        if let Some(weblink) = pending.remove(token) {
            let elapsed = now_secs().saturating_sub(weblink.created_at);
            if elapsed < WEBLINK_EXPIRY_SECS {
                let chat_id = weblink.chat_id;
                drop(pending); // release lock before acquiring sessions lock
                self.create_session(chat_id);
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Remove the session for a chat ID.
    pub fn logout(&self, chat_id: i64) {
        let mut sessions = lock_mutex(&self.sessions);
        sessions.remove(&chat_id);
    }

    /// Try to authenticate a chat ID using the given input.
    /// Dispatches to the correct verification method based on the configured 2FA method.
    /// Returns true if authentication succeeded.
    pub fn try_authenticate(&self, chat_id: i64, input: &str) -> bool {
        match &self.method {
            TwoFactorMethod::None => true,
            TwoFactorMethod::Totp { .. } => self.verify_totp(chat_id, input.trim()),
            TwoFactorMethod::Password { .. } => self.verify_password(chat_id, input.trim()),
            TwoFactorMethod::Weblink => {
                // Weblink auth is confirmed externally via confirm_weblink().
                // The user cannot authenticate by sending a message.
                false
            }
        }
    }

    /// Hash a password using argon2. Returns the PHC-formatted hash string.
    pub fn hash_password(password: &str) -> String {
        use argon2::PasswordHasher;
        let salt = argon2::password_hash::SaltString::generate(
            &mut argon2::password_hash::rand_core::OsRng,
        );
        let argon2 = argon2::Argon2::default();
        argon2
            .hash_password(password.as_bytes(), &salt)
            .expect("argon2 hashing should not fail")
            .to_string()
    }

    /// Generate a TOTP secret. Returns (base32_secret, otpauth_uri).
    pub fn generate_totp_secret(issuer: &str) -> (String, String) {
        let secret = totp_rs::Secret::generate_secret();
        let secret_base32 = secret.to_encoded().to_string();
        let secret_bytes = secret.to_bytes().expect("generated secret should be valid");
        let totp = totp_rs::TOTP::new(
            totp_rs::Algorithm::SHA1,
            6,
            1,
            30,
            secret_bytes,
            Some(issuer.to_string()),
            issuer.to_string(),
        )
        .expect("TOTP creation should not fail with valid secret");
        let uri = totp.get_url();
        (secret_base32, uri)
    }

    /// Check if 2FA is enrolled (method is not None).
    pub fn is_enrolled(&self) -> bool {
        !matches!(self.method, TwoFactorMethod::None)
    }

    /// Create a session for a chat ID.
    fn create_session(&self, chat_id: i64) {
        let mut sessions = lock_mutex(&self.sessions);
        sessions.insert(
            chat_id,
            Session {
                authenticated_at: now_secs(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_2fa_always_authenticated() {
        let auth = TwoFactorAuth::new(TwoFactorMethod::None);
        assert!(auth.is_authenticated(12345));
        assert!(auth.is_authenticated(99999));
        assert!(!auth.requires_challenge());
    }

    #[test]
    fn test_password_not_authenticated_initially() {
        let hash = TwoFactorAuth::hash_password("secret123");
        let auth = TwoFactorAuth::new(TwoFactorMethod::Password { hash });
        assert!(!auth.is_authenticated(12345));
        assert!(auth.requires_challenge());
    }

    #[test]
    fn test_password_authenticate_correct() {
        let hash = TwoFactorAuth::hash_password("mypassword");
        let auth = TwoFactorAuth::new(TwoFactorMethod::Password { hash });
        assert!(!auth.is_authenticated(42));
        assert!(auth.try_authenticate(42, "mypassword"));
        assert!(auth.is_authenticated(42));
    }

    #[test]
    fn test_password_authenticate_wrong() {
        let hash = TwoFactorAuth::hash_password("correct_password");
        let auth = TwoFactorAuth::new(TwoFactorMethod::Password { hash });
        assert!(!auth.try_authenticate(42, "wrong_password"));
        assert!(!auth.is_authenticated(42));
    }

    #[test]
    fn test_totp_generates_secret() {
        let (secret, uri) = TwoFactorAuth::generate_totp_secret("NyayaAgent");
        assert!(!secret.is_empty());
        assert!(uri.contains("otpauth://totp/"));
        assert!(uri.contains("NyayaAgent"));
    }

    #[test]
    fn test_session_expiry() {
        let hash = TwoFactorAuth::hash_password("pass");
        let mut auth = TwoFactorAuth::new(TwoFactorMethod::Password { hash });
        auth.session_ttl_secs = 0; // immediate expiry
        assert!(auth.try_authenticate(42, "pass"));
        // Session should be expired immediately
        assert!(!auth.is_authenticated(42));
    }

    #[test]
    fn test_logout_clears_session() {
        let hash = TwoFactorAuth::hash_password("pass");
        let auth = TwoFactorAuth::new(TwoFactorMethod::Password { hash });
        assert!(auth.try_authenticate(42, "pass"));
        assert!(auth.is_authenticated(42));
        auth.logout(42);
        assert!(!auth.is_authenticated(42));
    }

    #[test]
    fn test_weblink_token_generation() {
        let auth = TwoFactorAuth::new(TwoFactorMethod::Weblink);
        let token = auth.generate_weblink_token(42);
        assert!(!token.is_empty());
        // UUID v4 format: 8-4-4-4-12
        assert_eq!(token.len(), 36);
        assert!(auth.confirm_weblink(&token));
        assert!(auth.is_authenticated(42));
    }

    #[test]
    fn test_weblink_token_wrong() {
        let auth = TwoFactorAuth::new(TwoFactorMethod::Weblink);
        let _token = auth.generate_weblink_token(42);
        assert!(!auth.confirm_weblink("wrong-token-value"));
        assert!(!auth.is_authenticated(42));
    }

    #[test]
    fn test_challenge_prompts() {
        let auth_none = TwoFactorAuth::new(TwoFactorMethod::None);
        assert!(auth_none.challenge_prompt().is_empty());

        let auth_totp = TwoFactorAuth::new(TwoFactorMethod::Totp {
            secret: "JBSWY3DPEHPK3PXP".to_string(),
        });
        assert!(auth_totp.challenge_prompt().contains("TOTP"));

        let hash = TwoFactorAuth::hash_password("x");
        let auth_pass = TwoFactorAuth::new(TwoFactorMethod::Password { hash });
        assert!(auth_pass.challenge_prompt().contains("password"));

        let auth_web = TwoFactorAuth::new(TwoFactorMethod::Weblink);
        assert!(auth_web.challenge_prompt().contains("web dashboard"));
    }

    #[test]
    fn test_weblink_try_authenticate_returns_false() {
        // Weblink cannot be authenticated via try_authenticate (needs confirm_weblink)
        let auth = TwoFactorAuth::new(TwoFactorMethod::Weblink);
        assert!(!auth.try_authenticate(42, "anything"));
    }

    #[test]
    fn test_is_enrolled_none() {
        let auth = TwoFactorAuth::new(TwoFactorMethod::None);
        assert!(!auth.is_enrolled());
    }

    #[test]
    fn test_is_enrolled_totp() {
        let auth = TwoFactorAuth::new(TwoFactorMethod::Totp {
            secret: "JBSWY3DPEHPK3PXP".to_string(),
        });
        assert!(auth.is_enrolled());
    }

    #[test]
    fn test_is_enrolled_password() {
        let hash = TwoFactorAuth::hash_password("somepassword");
        let auth = TwoFactorAuth::new(TwoFactorMethod::Password { hash });
        assert!(auth.is_enrolled());
    }

    #[test]
    fn test_is_enrolled_weblink() {
        let auth = TwoFactorAuth::new(TwoFactorMethod::Weblink);
        assert!(auth.is_enrolled());
    }
}
