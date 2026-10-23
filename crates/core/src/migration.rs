//! Read-only compatibility with the previous application identity and ciphertexts.
//! All new files and ciphertexts use Noryn names; never write the old format.
pub const PREVIOUS_APP_ID: &str = "dev.harbor.client";
pub const PREVIOUS_VAULT_CONTEXT: &[u8] = b"harbor:vault:v1";
pub fn previous_team_context(team: &str) -> String {
    format!("harbor:team:v1:{team}")
}
