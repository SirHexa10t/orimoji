//! One sentence folded into every alphabet: what each looks like, and what it costs.
//!
//! ```sh
//! cargo run --example alphabets
//! cargo run --example alphabets -- "any text of your own"
//! ```

use orimoji::Alphabet;

fn main() {
    let text =
        std::env::args().nth(1).unwrap_or_else(|| "Fold me into fewer characters.".to_string());
    println!("{} bytes: {text}\n", text.len());
    println!("{:<10} {:>5} {:>5} {:>5}  folded", "alphabet", "chars", "bytes", "cells");
    for alphabet in Alphabet::ALL {
        let folded = alphabet.fold_bytes(text.as_bytes());
        assert_eq!(alphabet.unfold_bytes(&folded).expect("it unfolds"), text.as_bytes());
        let (chars, cells) = (folded.chars().count(), orimoji::measure::cells(&folded));
        println!("{:<10} {chars:>5} {:>5} {cells:>5}  {folded}", alphabet.name(), folded.len());
    }
}
