//! Many humans, one signer host: one sealed store per account, each unlocked only by that
//! account's passkey-derived secret. A token is routed to its account's branch and verified
//! with that account's own Biscuit root; another human's passkey opens nothing here.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::{HumanKey, Provisioned, Signature, Signer, SignerError};

pub struct Keyring {
    dir: PathBuf,
    accounts: BTreeMap<String, Signer>,
}

fn safe(account: &str) -> Result<&str, SignerError> {
    let ok = !account.is_empty()
        && account.len() <= 128
        && account.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'@'))
        && !account.starts_with('.');
    ok.then_some(account).ok_or_else(|| SignerError::Denied("bad account id".into()))
}

impl Keyring {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into(), accounts: BTreeMap::new() }
    }

    fn path(&self, account: &str) -> Result<PathBuf, SignerError> {
        Ok(self.dir.join(format!("{}.sealed", safe(account)?)))
    }

    /// First passkey unlock of an account creates its branch.
    pub fn enroll(&mut self, account: &str, human: &HumanKey) -> Result<(), SignerError> {
        let mut s = Signer::init(self.path(account)?, human)?;
        s.set_account(account)?;
        self.accounts.insert(account.to_owned(), s);
        Ok(())
    }

    pub fn unlock(&mut self, account: &str, human: &HumanKey) -> Result<(), SignerError> {
        let mut s = Signer::open(self.path(account)?);
        s.unlock(human)?;
        if s.account()? != account {
            return Err(SignerError::WrongHumanKey);
        }
        self.accounts.insert(account.to_owned(), s);
        Ok(())
    }

    pub fn lock(&mut self, account: &str) {
        self.accounts.remove(account);
    }

    pub fn is_unlocked(&self, account: &str) -> bool {
        self.accounts.get(account).is_some_and(Signer::is_unlocked)
    }

    pub fn branch(&mut self, account: &str) -> Result<&mut Signer, SignerError> {
        self.accounts.get_mut(account).ok_or(SignerError::Locked)
    }

    /// Account named inside a token (unverified read, only for routing; verification happens
    /// in that account's branch with its own root key).
    pub fn account_of(token: &str) -> Result<String, SignerError> {
        let t = biscuit_auth::UnverifiedBiscuit::from_base64(token).map_err(|_| SignerError::BadToken)?;
        let src = t.print_block_source(0).map_err(|_| SignerError::BadToken)?;
        let start = src.find("account(\"").ok_or(SignerError::BadToken)? + 9;
        let end = src[start..].find('"').ok_or(SignerError::BadToken)? + start;
        Ok(src[start..end].to_owned())
    }

    /// The unlocked account whose tree contains `node`.
    pub fn account_for_node(&self, node: &str) -> Option<String> {
        self.accounts.iter().find(|(_, s)| s.has_node(node)).map(|(a, _)| a.clone())
    }

    pub fn sign_from_node(&self, node: &str, token: &str, op: &str, payload: &[u8]) -> Result<Signature, SignerError> {
        let account = Self::account_of(token)?;
        self.accounts.get(&account).ok_or(SignerError::Locked)?.sign_from_node(node, token, op, payload)
    }

    pub fn sign(&self, token: &str, op: &str, payload: &[u8]) -> Result<Signature, SignerError> {
        let account = Self::account_of(token)?;
        self.accounts.get(&account).ok_or(SignerError::Locked)?.sign(token, op, payload)
    }

    pub fn provision(&mut self, account: &str, node: &str, provider: &str, session: &str, rights: std::collections::BTreeSet<String>, human: Option<&HumanKey>) -> Result<Provisioned, SignerError> {
        self.branch(account)?.provision(node, provider, session, rights, human)
    }

    /// Public key of a session, for room members (public keys may be shared per room rules).
    pub fn public_key_of(&self, account: &str, session: &str) -> Result<[u8; 32], SignerError> {
        self.accounts.get(account).ok_or(SignerError::Locked)?.public_key_of(session)
    }
}
