mod application;
mod infrastructure;
mod controller;
mod domain;
mod repository;

use std::process::exit;
use clap::{Parser};
use crate::application::AppError;
use crate::domain::{ServiceError, TransactionService};
use crate::repository::{InMemAccountRepository, InMemTransactionRepository};

/// Application arguments.
#[derive(Parser, Debug)]
struct Arguments {
    /// The input file name containing transactions.
    input_filename: String,
}

fn run(arguments: Arguments) -> Result<(), ServiceError> {

    // Build the app by injecting dependencies.
    let account_repository = InMemAccountRepository::default();
    let transaction_repository = InMemTransactionRepository::default();
    let mut transaction_service = TransactionService::new(account_repository, transaction_repository);

    // Process the input file.
    transaction_service.process_transactions_from_file(arguments.input_filename)?;
    transaction_service.report_account_statuses()
}


fn main() -> Result<(), AppError> {
    // Run and handle program exit status.
    // Take arguments, if it fails to parse skip to USAGE.
    let arguments = Arguments::parse();

    match run(arguments) {
        Ok(_) => {
            exit(exitcode::OK);
        }
        Err(error) => {
            eprintln!("Process exited with errors: {:?}", error);
            match error {
                ServiceError::IOError(_) => exit(exitcode::IOERR),
                _ => exit(exitcode::DATAERR),
            }
        }
    }
}

#[cfg(feature = "integration-tests")]
mod test {

    use std::process::Command;
    use assert_cmd::prelude::{CommandCargoExt, OutputAssertExt};
    use predicates::prelude::predicate;

    #[test]
    fn usage_with_missing_arguments() -> Result<(), Box<dyn std::error::Error>> {
        let mut cmd = Command::cargo_bin("rails")?;
        cmd.assert()
           .failure()
           .stderr(predicate::str::contains("USAGE"));
        Ok(())
    }

    #[test]
    fn failure_with_invalid_arguments() -> Result<(), Box<dyn std::error::Error>> {
        let mut cmd = Command::cargo_bin("rails")?;
        cmd.arg("a_file_that_does_not_exist.csv");
        cmd.assert()
           .failure()
           .stderr(predicate::str::contains("No such file or directory"));
        Ok(())
    }

    #[test]
    fn happy_path() -> Result<(), Box<dyn std::error::Error>> {
        let mut cmd = Command::cargo_bin("rails")?;
        cmd.arg("transactions.csv");
        cmd.assert()
           .success();
        Ok(())
    }
}