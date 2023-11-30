use std::{io, result, fmt, error};

#[derive(Debug)]
pub enum Error {
    System(String),
    Io(io::Error),
    Message(serde_json::Error),
}

pub type Result<T> = result::Result<T, Error>;