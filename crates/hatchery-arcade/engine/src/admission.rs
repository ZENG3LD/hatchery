//! The typed boundary between "energy earned outside" and "runs available
//! inside." The engine never sees a provider, a token count, a session, or
//! `gate4agent` at all -- only [`AdmissionSource`], implemented entirely by
//! the host. The host converts whatever real consumption signal it trusts
//! into a plain `u32` before ever calling [`crate::runner::Runner::start`]
//! -- no provider id, no token count, no session key crosses into this
//! crate.
//!
//! **On the file-list header's `RunTicket`**: the plan's own file-layout
//! comment for this module names `AdmissionCredit, AdmissionSource,
//! AdmissionError, RunTicket`, but the plan's own detailed code section
//! for `admission.rs` never defines or consumes a `RunTicket` type --
//! `Runner::start`'s contract is fully satisfied by `try_debit`'s
//! `Result<(), AdmissionError>` alone. Per this workspace's own "no
//! field/type nothing reads" convention, no such type is fabricated here;
//! if a future pass needs a debit receipt (e.g. to refund an aborted
//! run), it should be added then, with a real consumer.

/// A whole-number unit of admission credit. The exchange rate between real
/// consumption signals and this type is entirely the host's own decision.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct AdmissionCredit(pub u32);

#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionError {
    #[error("insufficient admission credit: need {required:?}, have {available:?}")]
    InsufficientCredit { required: AdmissionCredit, available: AdmissionCredit },
}

/// Implemented entirely by the host. The engine debits admission exactly
/// once, at [`crate::runner::Runner::start`] -- "energy gates admission,
/// never in-run power."
pub trait AdmissionSource {
    fn available(&self) -> AdmissionCredit;
    fn try_debit(&mut self, cost: AdmissionCredit) -> Result<(), AdmissionError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::CounterSource;

    #[test]
    fn try_debit_succeeds_and_reduces_balance() {
        let mut source = CounterSource(10);
        assert!(source.try_debit(AdmissionCredit(3)).is_ok());
        assert_eq!(source.available(), AdmissionCredit(7));
    }

    #[test]
    fn try_debit_rejects_when_balance_is_insufficient() {
        let mut source = CounterSource(2);
        let err = source.try_debit(AdmissionCredit(3)).unwrap_err();
        assert_eq!(
            err,
            AdmissionError::InsufficientCredit { required: AdmissionCredit(3), available: AdmissionCredit(2) }
        );
        assert_eq!(source.available(), AdmissionCredit(2));
    }
}
