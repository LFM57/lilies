#![deny(unsafe_code)]

use std::io;

/// Errors that can occur during network or resource fetching.
#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("Unsupported URL scheme: {0}")]
    UnsupportedScheme(String),

    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    #[error("HTTP error {status}: {message}")]
    Http { status: u16, message: String },

    #[error("Network error: {0}")]
    Network(String),

    #[error("Too many redirects (exceeded limit of 5)")]
    TooManyRedirects,

    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

impl From<ureq::Error> for NetError {
    fn from(err: ureq::Error) -> Self {
        match err {
            ureq::Error::Status(status, resp) => NetError::Http {
                status,
                message: resp.status_text().to_string(),
            },
            ureq::Error::Transport(transport) => {
                if transport.kind() == ureq::ErrorKind::TooManyRedirects {
                    NetError::TooManyRedirects
                } else {
                    NetError::Network(transport.to_string())
                }
            }
        }
    }
}
