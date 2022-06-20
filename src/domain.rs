use std::io;
use std::ops::{Add, Sub};
use std::path::Path;
use thiserror::Error;
use crate::infrastructure::{ReportProducer, TransactionFileReader};

use bigdecimal::{BigDecimal, Signed, Zero};
use serde::Deserialize;
use crate::ServiceError::GenericErrorMsg;

/// Type definitions for correctness and clean code.
pub type ClientId = u64;
pub type TransactionId = u64;
pub type Amount = BigDecimal;

const ROUND_DIGITS: i64 = 4;

/// The set of operations the process expects to find in the transactions file.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum Operation {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback,
}

/// Request describing an attempt to execute a transaction.
#[derive(Debug, Deserialize, Clone)]
pub struct TransactionRequest {
    #[serde(rename = "type")]
    transaction_type: Option<Operation>,
    #[serde(rename = "client")]
    client_id: Option<ClientId>,
    #[serde(rename = "tx")]
    transaction_id: Option<TransactionId>,
    #[serde(rename = "amount")]
    amount: Option<Amount>,
}

#[derive(Debug, Clone)]
pub enum TransactionStatus {
    /// Received transactions are stored in pending state until it is applied to the account.
    Pending,
    /// Once successfully applied to the account will receive an Applied status.
    Applied,
    ///  In case there is an inconsistency the transaction is marked as Error.
    ///  Transactions in this status usually need to be manually amended by an operator.
    ///  Also there are cases where transactions are not received in order and in some scenarios
    /// might apply withdrawals beyond the available amount, does not mean that the transaction is
    /// invalid, the system became temporarily inconsistent and re-execution of Error(ed) transactions
    /// later might solve this status without human intervention.
    Error,
}

#[derive(Debug, Clone)]
pub enum TransactionDispute {
    No,
    Disputed,
    Resolved,
    Chargeback,
}

#[derive(Debug, Clone)]
pub struct Transaction {
    operation: Operation,
    client_id: ClientId,
    transaction_id: TransactionId,
    amount: Option<Amount>,
    status: TransactionStatus,
    dispute: TransactionDispute,
}

impl Transaction {
    pub fn transaction_id(&self) -> TransactionId { self.transaction_id }
    pub fn amount(&self) -> Option<Amount> { self.amount.clone() }
    pub fn set_status(&mut self, status: TransactionStatus) {
        self.status = status;
    }
    pub fn set_dispute(&mut self, dispute: TransactionDispute) {
        self.dispute = dispute;
    }
}

impl From<TransactionRequest> for Transaction {
    // The From trait has no place to catch errors, this impl is intended to panic if the data is
    // invalid.
    fn from(request: TransactionRequest) -> Self {
        Transaction {
            operation: request.transaction_type.unwrap(),
            client_id: request.client_id.unwrap(),
            transaction_id: request.transaction_id.unwrap(),
            amount: match request.amount {
                None => None,
                Some(amount) => { Some(amount.round(ROUND_DIGITS)) }
            },
            status: TransactionStatus::Pending,
            dispute: TransactionDispute::No
        }
    }
}

impl TransactionRequest {
    pub fn valid_transaction(&self) -> Result<Transaction, ServiceError> {
        if self.amount.is_some() {
            let amount_opt = self.amount.to_owned();
            let val: BigDecimal = amount_opt.unwrap();
            if val.round(ROUND_DIGITS).ne(&val.clone()) {
                return Err(ServiceError::GenericErrorMsg("Invalid transaction amount.".to_string()));
            }
        }
        match self.transaction_type.is_some() && self.transaction_id.is_some() {
            true => Ok(Transaction::from(self.to_owned())),
            false => Err(ServiceError::GenericErrorMsg("Invalid transaction request.".to_string()))
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Account {
    client_id: ClientId,
    available: Amount,
    held: Amount,
    locked: bool,
    last_tx_applied: Option<TransactionId>,
}

impl Account {
    pub fn new(client_id: ClientId) -> Self {
        Account {
            client_id,
            available: BigDecimal::zero(),
            held: BigDecimal::zero(),
            locked: false,
            last_tx_applied: None,
        }
    }
    pub fn client_id(&self) -> ClientId { self.client_id }

    /// The total funds that are available for trading, staking, withdrawal, etc.
    /// This should be equal to the total - held amounts
    pub fn available(&self) -> Amount { self.available.clone() }

    /// The total funds that are held for dispute.
    /// This should be equal to total - available amounts
    pub fn held(&self) -> Amount { self.held.clone() }

    /// Total is an aggregate, is the sum of the available + held funds.
    pub fn total(&self) -> Amount {
        self.available.clone().add(&self.held).round(ROUND_DIGITS)
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }
}

/// Repository errors.
#[derive(Error, Debug)]
pub enum ServiceError {
    #[error("Error, {0}")]
    GenericErrorMsg(String),

    #[error("IO Error")]
    IOError(#[from] io::Error),

    #[error("Data Error")]
    DataError(#[from] RepositoryError),

}

#[derive(Error, Debug)]
pub enum RepositoryError {
    #[error("Entity already exists with id: {0}")]
    EntityAlreadyExists(String),

    #[error("Entity with id was not found: {0}")]
    EntityNotFound(String),

    #[error("Inconsistency detected, reference: {0}")]
    InconsistencyDetected(String),

}

pub struct TransactionService<AccRep, TxRep>
    where AccRep: AccountRepository, {
    account_repository: AccRep,
    transaction_repository: TxRep,
}

/// Kind of a business util function. Sanitizes the transaction amount by checking preconditions.
fn sanitize_transaction_amount(transaction: &Transaction) -> Result<Amount, ServiceError> {
    match transaction.amount.to_owned() {
        None => Err(GenericErrorMsg(std::format!("Invalid deposit tx: {}", transaction.transaction_id()))),
        Some(amount) => {
            if amount.is_negative() {
                Err(GenericErrorMsg(std::format!("Invalid deposit tx: {}", transaction.transaction_id())))
            } else {
                Ok(amount)
            }
        }
    }
}

impl<AccRep, TxRep> TransactionService<AccRep, TxRep>
    where AccRep: AccountRepository,
          TxRep: TransactionRepository, {
    pub fn new(account_repository: AccRep, transaction_repository: TxRep) -> Self {
        TransactionService {
            account_repository,
            transaction_repository,
        }
    }

    pub fn process_transactions_from_file<F>(&mut self, filename: F) -> Result<(), ServiceError>
        where F: AsRef<Path> {
        let mut reader = TransactionFileReader::from(filename)?;
        self.process_transactions(reader.values())
    }

    pub fn report_account_statuses(&mut self) -> Result<(), ServiceError> {
        let mut report = ReportProducer::new();
        let f = |account : &Account| {
            report.add(&account);
        };
        self.account_repository
            .account_visitor(f)
            .or_else( |err| Err(GenericErrorMsg(format!("Error accessing account repository. {:?}", err))))
    }
    pub fn process_transactions(&mut self, transaction_iter: impl Iterator<Item=TransactionRequest>) -> Result<(), ServiceError> {
        for tx in transaction_iter {
            match self.process_transaction(tx) {
                Ok(_) => (),
                Err(err) => {
                    // We want to continue processing other transactions so just notify the error
                    // and continue.
                    eprintln!("{:?}",err)
                }
            }

        }
        Ok(())
    }

    ///  This method initiates the transaction execution by checking minimum preconditions and then
    /// delegates the rest of the execution to the corresponding method.
    pub fn process_transaction(&mut self, request: TransactionRequest) -> Result<(), ServiceError> {

        // Obtain a valid transaction from the request or err.
        let transaction = request.valid_transaction()?;

        match transaction.operation {
            Operation::Deposit | Operation::Withdrawal => {
                // Check if we have already processed the transaction using the transaction id for idempotency.
                // Note that the transaction status is Pending.
                self.transaction_repository.post_transaction(&transaction)?;
            }
            Operation::Dispute | Operation::Resolve | Operation::Chargeback => {
                //  For correctness, all cases need to be declared, in this case these transactions
                // are not posted to the transaction repository, they must be already present.
            }
        }

        // Dispatch the operation.
        let transaction_status = match &transaction.operation {
            Operation::Deposit => {
                self.process_deposit(&transaction)
            }
            Operation::Withdrawal => {
                self.process_withdrawal(&transaction)
            }
            Operation::Dispute => {
                self.process_dispute(&transaction)
            }
            Operation::Resolve => {
                self.process_resolve(&transaction)
            }
            Operation::Chargeback => {
                self.process_chargeback(&transaction)
            }
        }?; // Fail in case of a repository problem.

        // Mark the transaction resolution status, from pending to the target status.
        self.transaction_repository.update_transaction_status(&transaction.transaction_id(), &transaction_status)?;

        Ok(())
    }

    fn process_chargeback(&mut self, transaction: &Transaction) -> Result<TransactionStatus, ServiceError> {

        // Must have a valid account.
        let account = self.account_repository.get_account(&transaction.client_id)?;

        if account.locked {
            return Err(GenericErrorMsg(format!("The requested account is locked and cannot process chargebacks. {}", transaction.transaction_id)));
        }

        // The reference transaction must exist
        let ref_transaction_opt = self.transaction_repository.find_transaction_by_id(&transaction.transaction_id)?;
        let ref_transaction = match ref_transaction_opt {
            None => return Err(GenericErrorMsg(format!("The requested transaction id does not exist. {}", transaction.transaction_id))),
            Some(ref_transaction) => {
                if !matches!(ref_transaction.dispute, TransactionDispute::Disputed) {
                    return Err(GenericErrorMsg(format!("The requested transaction is not disputed. {}", transaction.transaction_id)));
                } else {
                    ref_transaction
                }
            }
        };
        // The reference transaction preconditions are that this tx must be in "Disputed" status.

        let amount_opt = ref_transaction.amount.to_owned();
        let amount = match amount_opt {
            None => return Err(GenericErrorMsg(format!("The requested transaction id does not specify an amount. {}", transaction.transaction_id))),
            Some(amount) => {
                if amount.gt(&account.held) {
                    return Ok(TransactionStatus::Error);
                } else {
                    amount
                }
            }
        };

        let mut update = account.clone();
        update.held = update.held.sub(&amount).round(ROUND_DIGITS);
        update.last_tx_applied = Some(transaction.transaction_id);
        update.locked = true;
        self.account_repository.update_account(&account, &update)?;

        self.transaction_repository.update_transaction_dispute(&ref_transaction.transaction_id, &TransactionDispute::Chargeback)?;

        Ok(TransactionStatus::Applied)
    }

    fn process_resolve(&mut self, transaction: &Transaction) -> Result<TransactionStatus, ServiceError> {
        let account = self.account_repository.get_account(&transaction.client_id)?;

        if account.locked {
            return Err(GenericErrorMsg(format!("The requested account is locked and cannot process further. {}", transaction.transaction_id)));
        }

        let ref_transaction_opt = self.transaction_repository.find_transaction_by_id(&transaction.transaction_id)?;

        let ref_transaction = match ref_transaction_opt {
            None => return Err(GenericErrorMsg(format!("The requested transaction id does not exist. {}", transaction.transaction_id))),
            Some(ref_transaction) => {
                if !matches!(ref_transaction.dispute, TransactionDispute::Disputed) {
                    return Err(GenericErrorMsg(format!("The requested transaction is not disputed. {}", transaction.transaction_id)));
                } else {
                    ref_transaction
                }
            }
        };

        let amount_opt = ref_transaction.amount.to_owned();
        let amount = match amount_opt {
            None => return Err(GenericErrorMsg(format!("The requested transaction id does not specify an amount. {}", transaction.transaction_id))),
            Some(amount) => {
                if amount.gt(&account.held) {
                    return Ok(TransactionStatus::Error);
                } else {
                    amount
                }
            }
        };

        let mut update = account.clone();
        update.available = update.available.add(&amount).round(ROUND_DIGITS);
        update.held = update.held.sub(&amount).round(ROUND_DIGITS);
        update.last_tx_applied = Some(transaction.transaction_id);
        self.account_repository.update_account(&account, &update)?;

        self.transaction_repository.update_transaction_dispute(&ref_transaction.transaction_id, &TransactionDispute::Resolved)?;

        Ok(TransactionStatus::Applied)
    }

    fn process_dispute(&mut self, transaction: &Transaction) -> Result<TransactionStatus, ServiceError> {
        let account = self.account_repository.get_account(&transaction.client_id)?;

        if account.locked {
            return Err(GenericErrorMsg(format!("The requested account is locked and cannot process further. {}", transaction.transaction_id)));
        }

        let ref_transaction_opt = self.transaction_repository.find_transaction_by_id(&transaction.transaction_id)?;
        let (ref_transaction, amount) = match ref_transaction_opt {
            None => return Err(GenericErrorMsg(format!("The requested transaction id does not exist. {}", transaction.transaction_id))),
            Some(ref_transaction) => {
                match ref_transaction.dispute {
                    TransactionDispute::Disputed | TransactionDispute::Chargeback => {
                        return Err(GenericErrorMsg(format!("The requested transaction id is in dispute status and is immutable until resolution. {}", transaction.transaction_id)));
                    }
                    TransactionDispute::No | TransactionDispute::Resolved => (),
                };

                let amount = match ref_transaction.amount() {
                    // This should never happen, if this happens the repository is corrupted.
                    None => return Err(GenericErrorMsg(format!("The requested transaction id does not specify an amount. {}", transaction.transaction_id))),
                    Some(amount) => {
                        if amount.gt(&account.available) {
                            return Ok(TransactionStatus::Error);
                        }
                        amount
                    }
                };
                (ref_transaction, amount)
            }
        };

        let mut update = account.clone();
        update.available = update.available.sub(&amount).round(ROUND_DIGITS);
        update.held = update.held.add(&amount).round(ROUND_DIGITS);
        update.last_tx_applied = Some(transaction.transaction_id);
        self.account_repository.update_account(&account, &update)?;

        self.transaction_repository.update_transaction_dispute(&ref_transaction.transaction_id, &TransactionDispute::Disputed)?;

        Ok(TransactionStatus::Applied)
    }

    fn process_withdrawal(&mut self, transaction: &Transaction) -> Result<TransactionStatus, ServiceError> {
        let amount = sanitize_transaction_amount(&transaction)?;
        let account = self.account_repository.get_account(&transaction.client_id)?;

        if account.locked {
            return Err(GenericErrorMsg(format!("The requested account is locked and cannot process withdrawals. {}", transaction.transaction_id)));
        }

        // We reject the withdrawal as it is not applicable to our view of the balance.
        if amount.gt(&account.available) {
            return Ok(TransactionStatus::Error);
        }
        let mut update = account.clone();
        update.available = update.available.sub(&amount).round(ROUND_DIGITS);
        update.last_tx_applied = Some(transaction.transaction_id);
        self.account_repository.update_account(&account, &update)?;
        Ok(TransactionStatus::Applied)
    }

    fn process_deposit(&mut self, transaction: &Transaction) -> Result<TransactionStatus, ServiceError> {
        let amount = sanitize_transaction_amount(&transaction)?;

        let account = self.account_repository.get_account(&transaction.client_id)?;

        if account.locked {
            return Err(GenericErrorMsg(format!("The requested account is locked and cannot process deposits. {}", transaction.transaction_id)));
        }

        let mut update = account.clone();
        update.available = update.available.add(&amount).round(ROUND_DIGITS);
        update.last_tx_applied = Some(transaction.transaction_id);
        self.account_repository.update_account(&account, &update)?;
        Ok(TransactionStatus::Applied)
    }

    #[allow(dead_code)]
    pub fn get_account_status(&mut self, client_id: &ClientId) -> Result<Account, ServiceError> {
        match self.account_repository.get_account(client_id) {
            Ok(account) => { Ok(account) }
            Err(e) => { Err(ServiceError::DataError(e)) }
        }
    }
}

pub trait AccountRepository {
    /// Always returns an account, if the account does not exists it is created.
    fn get_account(&mut self, client_id: &ClientId) -> Result<Account, RepositoryError>;

    /// Do a CAS update validating that the account is in the state we believe it is before applying
    /// changes. This is useful in optimistic locking mechanisms at shared remote repositories.
    fn update_account(&mut self, account: &Account, update: &Account) -> Result<(), RepositoryError>;

    fn account_visitor<F>(&mut self, f: F) -> Result<(), RepositoryError> where F: FnMut(&Account);
}

pub trait TransactionRepository {
    /// Attempt to post the transaction into the repository. Fails if the transaction already exists.
    fn post_transaction(&mut self, transaction: &Transaction) -> Result<(), RepositoryError>;

    /// Updates the target transaction id status.
    fn update_transaction_status(&mut self, transaction_id: &TransactionId, status: &TransactionStatus) -> Result<(), RepositoryError>;

    /// Updates the target transaction id dispute.
    fn update_transaction_dispute(&mut self, transaction_id: &TransactionId, dispute: &TransactionDispute) -> Result<(), RepositoryError>;

    /// Optionally find a transaction by id.
    fn find_transaction_by_id(&mut self, transaction_id: &TransactionId) -> Result<Option<Transaction>, RepositoryError>;
}

#[cfg(test)]
mod test {
    use std::str::FromStr;
    use mockall::mock;
    use mockall::predicate::*;

    use crate::domain::*;
    use crate::{InMemAccountRepository, InMemTransactionRepository, TransactionService};

    mock! {
        pub TransactionRepo {}
        impl TransactionRepository for TransactionRepo {
            fn post_transaction(&mut self, transaction: &Transaction) -> Result<(), RepositoryError>;
            fn update_transaction_status(&mut self, transaction_id: &TransactionId, status: &TransactionStatus) -> Result<(), RepositoryError>;
            fn find_transaction_by_id(&mut self, transaction_id: &TransactionId) -> Result<Option<Transaction>, RepositoryError>;
            fn update_transaction_dispute(&mut self, transaction_id: &TransactionId, dispute: &TransactionDispute) -> Result<(), RepositoryError>;
        }
    }

    #[test]
    fn reject_duplicated_transaction() {
        let acc_repo = InMemAccountRepository::default();
        let tx_repo = InMemTransactionRepository::default();
        let mut transaction_service = TransactionService::new(acc_repo, tx_repo);
        let result = transaction_service.process_transaction(TransactionRequest {
            transaction_type: Some(Operation::Deposit),
            client_id: Some(1),
            transaction_id: Some(1),
            amount: Some(BigDecimal::from_str("1.2345").unwrap()),
        });
        assert!(result.is_ok());
        let result = transaction_service.process_transaction(TransactionRequest {
            transaction_type: Some(Operation::Deposit),
            client_id: Some(1),
            transaction_id: Some(1),
            amount: Some(BigDecimal::from_str("1.2345").unwrap()),
        });
        assert!(result.is_err());
        let err = result.map_err(|e| match e {
            ServiceError::DataError(e) => match e {
                RepositoryError::EntityAlreadyExists(_) => true,
                _ => false,
            },
            _ => false
        });
        assert!(err.err().unwrap());

        let result = transaction_service.process_transaction(TransactionRequest {
            transaction_type: Some(Operation::Deposit),
            client_id: Some(1),
            transaction_id: Some(2),
            amount: Some(BigDecimal::from_str("1.2345").unwrap()),
        });
        assert!(result.is_ok());
    }

    #[test]
    fn reject_invalid_transaction_request_variations() {
        let acc_repo = InMemAccountRepository::default();
        let tx_repo = InMemTransactionRepository::default();
        let mut transaction_service = TransactionService::new(acc_repo, tx_repo);

        // No transaction id.
        let result = transaction_service.process_transaction(TransactionRequest {
            transaction_type: Some(Operation::Deposit),
            client_id: Some(1),
            transaction_id: None,
            amount: Some(BigDecimal::from_str("1.2345").unwrap()),
        });
        assert!(result.is_err());

        // Invalid number of decimal positions.
        let result = transaction_service.process_transaction(TransactionRequest {
            transaction_type: Some(Operation::Deposit),
            client_id: Some(1),
            transaction_id: Some(1),
            amount: Some(BigDecimal::from_str("1.23456").unwrap()),
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_accept_valid_transactions() -> Result<(), Box<dyn std::error::Error>> {
        let acc_repo = InMemAccountRepository::default();
        let tx_repo = InMemTransactionRepository::default();
        let mut transaction_service = TransactionService::new(acc_repo, tx_repo);
        let valid_transactions = vec![
            TransactionRequest {
                transaction_type: Some(Operation::Deposit),
                transaction_id: Some(1),
                client_id: Some(1),
                amount: Some(BigDecimal::from_str("1.1234").unwrap()),
            },
            TransactionRequest {
                transaction_type: Some(Operation::Deposit),
                transaction_id: Some(2),
                client_id: Some(1),
                amount: Some(BigDecimal::from_str("1.1234").unwrap()),
            },
            TransactionRequest {
                transaction_type: Some(Operation::Withdrawal),
                transaction_id: Some(3),
                client_id: Some(1),
                amount: Some(BigDecimal::from_str("1.1234").unwrap()),
            },
            TransactionRequest {
                transaction_type: Some(Operation::Dispute),
                transaction_id: Some(2),
                client_id: Some(1),
                amount: None,
            },
            TransactionRequest {
                transaction_type: Some(Operation::Resolve),
                transaction_id: Some(2),
                client_id: Some(1),
                amount: None,
            },
        ];
        let result = transaction_service.process_transactions(valid_transactions.into_iter());
        assert!(result.is_ok());

        let client_id: ClientId = 1;
        let account = transaction_service.get_account_status(&client_id).unwrap();
        assert_eq!(BigDecimal::from_str("1.1234").unwrap(), account.available());
        assert_eq!(BigDecimal::from_str("0.0000").unwrap(), account.held());
        assert_eq!(BigDecimal::from_str("1.1234").unwrap(), account.total());
        Ok(())
    }

    #[test]
    fn test_chargeback() -> Result<(), Box<dyn std::error::Error>> {
        let acc_repo = InMemAccountRepository::default();
        let tx_repo = InMemTransactionRepository::default();
        let mut transaction_service = TransactionService::new(acc_repo, tx_repo);
        let valid_transactions = vec![
            TransactionRequest {
                transaction_type: Some(Operation::Deposit),
                transaction_id: Some(1),
                client_id: Some(1),
                amount: Some(BigDecimal::from_str("1.1234").unwrap()),
            },
            TransactionRequest {
                transaction_type: Some(Operation::Dispute),
                transaction_id: Some(1),
                client_id: Some(1),
                amount: None,
            },
            TransactionRequest {
                transaction_type: Some(Operation::Chargeback),
                transaction_id: Some(1),
                client_id: Some(1),
                amount: None,
            },
        ];
        transaction_service.process_transactions(valid_transactions.into_iter())?;

        let client_id: ClientId = 1;
        let account = transaction_service.get_account_status(&client_id).unwrap();
        assert_eq!(BigDecimal::from_str("0.0000").unwrap(), account.available());
        assert_eq!(BigDecimal::from_str("0.0000").unwrap(), account.held());
        assert_eq!(BigDecimal::from_str("0.0000").unwrap(), account.total());
        assert!(account.locked);
        Ok(())
    }

    #[test]
    fn test_locked() -> Result<(), Box<dyn std::error::Error>> {
        let acc_repo = InMemAccountRepository::default();
        let tx_repo = InMemTransactionRepository::default();
        let mut transaction_service = TransactionService::new(acc_repo, tx_repo);
        let valid_transactions = vec![
            TransactionRequest {
                transaction_type: Some(Operation::Deposit),
                transaction_id: Some(1),
                client_id: Some(1),
                amount: Some(BigDecimal::from_str("1.1234").unwrap()),
            },
            TransactionRequest {
                transaction_type: Some(Operation::Dispute),
                transaction_id: Some(1),
                client_id: Some(1),
                amount: None,
            },
            TransactionRequest {
                transaction_type: Some(Operation::Chargeback),
                transaction_id: Some(1),
                client_id: Some(1),
                amount: None,
            },

        ];
        transaction_service.process_transactions(valid_transactions.into_iter())?;

        let result = transaction_service.process_transaction(TransactionRequest {
            transaction_type: Some(Operation::Deposit),
            transaction_id: Some(2),
            client_id: Some(1),
            amount: Some(BigDecimal::from_str("1.1234").unwrap()),
        });

        assert!(result.is_err());

        let client_id: ClientId = 1;
        let account = transaction_service.get_account_status(&client_id).unwrap();
        assert_eq!(BigDecimal::from_str("0.0000").unwrap(), account.available());
        assert_eq!(BigDecimal::from_str("0.0000").unwrap(), account.held());
        assert_eq!(BigDecimal::from_str("0.0000").unwrap(), account.total());
        assert!(account.locked);
        Ok(())

    }

    #[test]
    fn test_report() -> Result<(), Box<dyn std::error::Error>> {
        let acc_repo = InMemAccountRepository::default();
        let tx_repo = InMemTransactionRepository::default();
        let mut transaction_service = TransactionService::new(acc_repo, tx_repo);
        let valid_transactions = vec![
            TransactionRequest {
                transaction_type: Some(Operation::Deposit),
                transaction_id: Some(1),
                client_id: Some(1),
                amount: Some(BigDecimal::from_str("1.1234").unwrap()),
            },
            TransactionRequest {
                transaction_type: Some(Operation::Deposit),
                transaction_id: Some(2),
                client_id: Some(1),
                amount: Some(BigDecimal::from_str("1.1234").unwrap()),
            },
            TransactionRequest {
                transaction_type: Some(Operation::Withdrawal),
                transaction_id: Some(3),
                client_id: Some(1),
                amount: Some(BigDecimal::from_str("1.1234").unwrap()),
            },
            TransactionRequest {
                transaction_type: Some(Operation::Dispute),
                transaction_id: Some(2),
                client_id: Some(1),
                amount: None,
            },
            TransactionRequest {
                transaction_type: Some(Operation::Resolve),
                transaction_id: Some(2),
                client_id: Some(1),
                amount: None,
            },
        ];
        transaction_service.process_transactions(valid_transactions.into_iter())?;
        transaction_service.report_account_statuses()?;
        Ok(())
    }

}
