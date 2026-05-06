use crate::app::account_profiles::load_profiles;

pub fn account_identity(profile_id: &str) -> String {
    if let Ok(profiles) = load_profiles() {
        if let Some(profile) = profiles.into_iter().find(|entry| entry.id == profile_id) {
            return account_identity_from_parts(profile_id, &profile.email);
        }
    }
    profile_id.to_string()
}

pub fn account_identity_from_parts(profile_id: &str, email: &str) -> String {
    let trimmed = email.trim();
    if trimmed.is_empty() {
        profile_id.to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn account_prefix(profile_id: &str) -> String {
    format!("[acct:{}]", account_identity(profile_id))
}

pub fn account_prefix_from_parts(profile_id: &str, email: &str) -> String {
    format!("[acct:{}]", account_identity_from_parts(profile_id, email))
}
