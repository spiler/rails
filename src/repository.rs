use std::collections::{HashMap};
use std::collections::hash_map::Entry;
use crate::domain::{AccountRepository, TransactionRepository, TransactionId, RepositoryError, ClientId, Account, Transaction, TransactionStatus, TransactionDispute};

pub struct InMemTransactionRepository {
    transactions_by_id: HashMap<TransactionId, Transaction>,
}

impl InMemTransactionRepository {
}

impl Default for InMemTransactionRepository {
    fn default() -> Self {
        InMemTransactionRepository{
            transactions_by_id: HashMap::default()
        }
    }
}

impl TransactionRepository for InMemTransactionRepository {
    fn post_transaction(&mut self, transaction: &Transaction) -> Result<(), RepositoryError> {
        match self.transactions_by_id.entry(transaction.transaction_id()) {
            Entry::Occupied(o) => {
                Err(RepositoryError::EntityAlreadyExists(o.key().to_string()))
            }
            Entry::Vacant(v) => {
                v.insert(transaction.to_owned());
                Ok(())
            }
        }
    }

    fn update_transaction_status(&mut self, transaction_id: &TransactionId, status: &TransactionStatus) -> Result<(), RepositoryError> {
        match self.transactions_by_id.entry(transaction_id.to_owned()) {
            Entry::Vacant(_) => Err(RepositoryError::EntityNotFound(transaction_id.to_string())),
            Entry::Occupied(mut o) => {
                let tx = o.get_mut();
                tx.set_status(status.to_owned());
                Ok(())
            }
        }
    }

    fn update_transaction_dispute(&mut self, transaction_id: &TransactionId, dispute: &TransactionDispute) -> Result<(), RepositoryError> {
        match self.transactions_by_id.entry(transaction_id.to_owned()) {
            Entry::Vacant(_) => Err(RepositoryError::EntityNotFound(transaction_id.to_string())),
            Entry::Occupied(mut o) => {
                let tx = o.get_mut();
                tx.set_dispute(dispute.to_owned());
                Ok(())
            }
        }
    }

    fn find_transaction_by_id(&mut self, transaction_id: &TransactionId) -> Result<Option<Transaction>, RepositoryError> {
       Ok(self.transactions_by_id.get(&transaction_id).cloned())
    }
}

pub struct InMemAccountRepository {
    accounts_by_client_id: HashMap<ClientId, Account>,
}

impl InMemAccountRepository {
}

impl Default for InMemAccountRepository {
    fn default() -> Self {
        InMemAccountRepository {
            accounts_by_client_id: HashMap::default()
        }
    }
}

impl AccountRepository for InMemAccountRepository {

    fn get_account(&mut self, client_id: &ClientId) -> Result<Account, RepositoryError> {
        match self.accounts_by_client_id.entry(client_id.clone()) {
            Entry::Occupied(occupied) => {
                Ok(occupied.get().clone())
            }
            Entry::Vacant(vacant) => {
                Ok(vacant.insert(Account::new(client_id.clone())).clone())
            }
        }
    }

    fn update_account(&mut self, account: &Account, update: &Account) -> Result<(), RepositoryError> {
        match self.accounts_by_client_id.entry(account.client_id()) {
            Entry::Occupied(mut o) => {
                // Do a CAS operation on what we believe is the last state of the account and what
                // we got from the repo.
               if o.get() == account {
                   o.insert(update.clone());
                   Ok(())
               } else {
                   Err(RepositoryError::InconsistencyDetected(format!("{}", account.client_id())))
               }
            }
            Entry::Vacant(_) => {
                Err(RepositoryError::EntityNotFound(format!("Account cannot be updated, it does not exist. {}", account.client_id())))
            }
        }

    }

    fn account_visitor<F>(&mut self, mut f: F) -> Result<(), RepositoryError> where F: FnMut(&Account) {
        self.accounts_by_client_id.values().for_each(|account| {f(account)});
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::domain::{AccountRepository, ClientId};
    use crate::InMemAccountRepository;

    #[test]
    fn account_created_on_get() {
        let mut repo = InMemAccountRepository::default();
        let client_id : ClientId = 1;
        let result = repo.get_account(&client_id);
        assert_eq!(result.unwrap().client_id(), 1);
    }
}