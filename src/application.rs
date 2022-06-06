use thiserror::Error;
use std::io;
use crate::ServiceError;

/// Generic application errors and conversion traits to communicate errors.
#[derive(Error, Debug)]
pub enum AppError {
    #[error("IO Error")]
    IOError(#[from] io::Error),

    #[error("Service Error!")]
    ServiceError(#[from] ServiceError),
}

