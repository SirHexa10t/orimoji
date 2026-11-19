//! Words from a fixed list, written as their places in it: a weekday in under 3 bits instead of
//! 57, and a record of several such fields as one number.
//!
//! ```sh
//! cargo run --example fixed_words
//! ```

use orimoji::Alphabet;
use orimoji::words::{Record, WordList};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let days = WordList::new([
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ])?;
    let months = WordList::new([
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ])?;
    let levels = WordList::new(["error", "warn", "info", "debug", "trace"])?;
    let methods = WordList::new([
        "GET", "HEAD", "POST", "PUT", "DELETE", "CONNECT", "OPTIONS", "TRACE", "PATCH",
    ])?;

    for (record, words) in [
        (Record::new(vec![&days]), vec!["Wednesday"]),
        (Record::new(vec![&days, &months]), vec!["Saturday", "November"]),
        (
            Record::new(vec![&levels, &methods, &days, &months]),
            vec!["warn", "DELETE", "Friday", "March"],
        ),
    ] {
        let spelled: usize = words.iter().map(|word| word.len() + 1).sum::<usize>() - 1;
        println!("{} ({spelled} bytes as text) takes {} bits:", words.join(" "), record.bits());
        for alphabet in [Alphabet::Hex, Alphabet::Base64Url, Alphabet::TwoByte, Alphabet::Cjk] {
            let folded = record.fold(&words, alphabet)?;
            assert_eq!(record.unfold(&folded, alphabet)?, words);
            println!("  {:<10} {folded}", alphabet.name());
        }
    }
    Ok(())
}
