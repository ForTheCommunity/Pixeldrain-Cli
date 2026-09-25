use std::fmt;

use serde::Deserialize;

#[derive( Deserialize,Debug)]
pub struct UploadResponse {
   pub id: String,
}

impl fmt::Display for UploadResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.id)
    }
}