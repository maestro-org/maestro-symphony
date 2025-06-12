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
