use bcrypt::{hash, verify, DEFAULT_COST, BcryptError};

pub fn hash_password(password: &str) -> Result<String, BcryptError> {
    hash(password, DEFAULT_COST)
}

pub fn verify_password(password: &str, hashed: &str) -> Result<bool, BcryptError> {
    verify(password, hashed)
}