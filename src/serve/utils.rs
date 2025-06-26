use std::str::FromStr;

use ordinals::{Rune, RuneId};

use crate::serve::error::ServeError;

pub fn decimal(num: u128, dec: u8) -> String {
    let dec = dec as usize;

    let mut bal_string = num.to_string();
    let bal_string_len = bal_string.len();

    if dec > 0 {
        if bal_string_len == dec {
            let mut new_string = String::from("0.");
            new_string.push_str(&bal_string);

            bal_string = new_string;
        } else if bal_string_len < dec {
            let mut new_string = String::from("0.");

            for _ in 0..(dec - bal_string_len) {
                new_string.push('0')
            }

            new_string.push_str(&bal_string);

            bal_string = new_string;
        } else {
            bal_string.insert(bal_string_len - dec, '.');
        }
    }

    bal_string
}

pub enum RuneIdentifier {
    Id(RuneId),
    Name(u128),
}

impl RuneIdentifier {
    pub fn parse(string: &str) -> Result<Self, ServeError> {
        if string.contains(':') {
            let parts: Vec<_> = string.split(':').collect();
            if parts.len() != 2 {
                return Err(ServeError::malformed_request(
                    "rune id must be etching block and transaction index in form '30562:50'",
                ));
            }
            let block = parts[0].parse().unwrap();
            let tx = parts[1].parse().unwrap();

            Ok(Self::Id(RuneId { block, tx }))
        } else {
            let without_spacers = string.replace("•", "");

            let rune = Rune::from_str(&without_spacers)
                .map_err(|_| ServeError::malformed_request("unable to decode rune name"))?;

            Ok(Self::Name(rune.n()))
        }
    }
}
