//! Many humans, one keystore host: one sealed branch per account, each unlocked only by that
//! account's passkey-derived secret ([`tesserax_keytree::MultiRoot`]).

use std::collections::BTreeSet;
use std::path::PathBuf;

use tesserax_keytree as kt;

use crate::{HumanKey, Provisioned, Signature, Signer, SignerError};

pub struct Keyring {
    dir: PathBuf,
    accounts: kt::MultiRoot<Signer>,
}

impl Keyring {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into(), accounts: kt::MultiRoot::new() }
    }

    fn path(&self, account: &str) -> Result<PathBuf, SignerError> {
        let id = kt::branch_id(account).map_err(|_| SignerError::Denied("bad account id".into()))?;
        Ok(self.dir.join(format!("{id}.sealed")))
    }

    pub fn enroll(&mut self, account: &str, human: &HumanKey) -> Result<(), SignerError> {
        let mut s = Signer::init(self.path(account)?, human)?;
        s.set_account(account)?;
        self.accounts.insert(account, s).map_err(|_| SignerError::Denied("bad account id".into()))
    }

    pub fn unlock(&mut self, account: &str, human: &HumanKey) -> Result<(), SignerError> {
        let mut s = Signer::open(self.path(account)?);
        s.unlock(human)?;
        if s.account()? != account {
            return Err(SignerError::WrongHumanKey);
        }
        self.accounts.insert(account, s).map_err(|_| SignerError::Denied("bad account id".into()))
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

    /// Account named inside a token (unverified, routing only).
    pub fn account_of(token: &str) -> Result<String, SignerError> {
        kt::unverified_fact(token, "account").map_err(|_| SignerError::BadToken)
    }

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

    pub fn delegate(&self, token: &str, transport: &str) -> Result<String, SignerError> {
        let account = Self::account_of(token)?;
        self.accounts.get(&account).ok_or(SignerError::Locked)?.delegate(token, transport)
    }

    pub fn provision(&mut self, account: &str, node: &str, provider: &str, session: &str, rights: BTreeSet<String>, human: Option<&HumanKey>) -> Result<Provisioned, SignerError> {
        self.branch(account)?.provision(node, provider, session, rights, human)
    }

    pub fn public_key_of(&self, account: &str, session: &str) -> Result<[u8; 32], SignerError> {
        self.accounts.get(account).ok_or(SignerError::Locked)?.public_key_of(session)
    }
}
