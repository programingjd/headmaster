use crate::errors::Error::IOError;

#[derive(Debug)]
pub enum Error {
    IOError(std::io::Error),
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        IOError(err)
    }
}
