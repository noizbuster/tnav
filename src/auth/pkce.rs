use oauth2::{CsrfToken, PkceCodeChallenge, PkceCodeVerifier};

#[derive(Debug)]
pub struct PkceBundle {
    pub state: String,
    pub code_challenge: PkceCodeChallenge,
    pub code_verifier: PkceCodeVerifier,
}

impl PkceBundle {
    pub fn generate() -> Self {
        let (code_challenge, code_verifier) = PkceCodeChallenge::new_random_sha256();

        Self {
            state: CsrfToken::new_random().secret().to_owned(),
            code_challenge,
            code_verifier,
        }
    }
}
