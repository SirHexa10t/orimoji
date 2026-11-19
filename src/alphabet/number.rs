//! The alphabets that read the whole input as one number and write its digits: base58 and base94.
//! A run of leading zero bytes, which a number cannot hold, is written as as many zero digits.
//! The work grows with the square of the length, so these suit short lines; a folded file's whole
//! body is folded in line-sized pieces for exactly that reason.

use super::Problem;
use crate::bignum::Nat;

pub(super) struct WholeNumber {
    pub(super) digits: &'static [u8],
    reverse: [u8; 128],
}

const fn reverse(digits: &[u8]) -> [u8; 128] {
    let mut table = [u8::MAX; 128];
    let mut i = 0;
    while i < digits.len() {
        table[digits[i] as usize] = i as u8;
        i += 1;
    }
    table
}

const BASE58_DIGITS: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const BASE94_DIGITS: &[u8] =
    b"!\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";

pub(super) const BASE58: WholeNumber =
    WholeNumber { digits: BASE58_DIGITS, reverse: reverse(BASE58_DIGITS) };
pub(super) const BASE94: WholeNumber =
    WholeNumber { digits: BASE94_DIGITS, reverse: reverse(BASE94_DIGITS) };

impl WholeNumber {
    pub(super) fn fold(&self, bytes: &[u8]) -> String {
        let zeros = bytes.iter().take_while(|&&byte| byte == 0).count();
        let mut number = Nat::from_be_bytes(&bytes[zeros..]);
        let base = self.digits.len() as u64;
        let mut digits = Vec::new();
        while !number.is_zero() {
            digits.push(self.digits[number.div_rem(base) as usize]);
        }
        let mut out = String::with_capacity(zeros + digits.len());
        out.extend(std::iter::repeat_n(self.digits[0] as char, zeros));
        out.extend(digits.iter().rev().map(|&digit| digit as char));
        out
    }

    pub(super) fn unfold(&self, text: &str) -> Result<Vec<u8>, (usize, Problem)> {
        let base = self.digits.len() as u64;
        let (mut zeros, mut number) = (0, Nat::zero());
        for (at, c) in text.chars().enumerate() {
            let digit = match self.reverse.get(c as usize) {
                Some(&digit) if digit != u8::MAX => digit,
                _ => return Err((at, Problem::Foreign(c))),
            };
            match digit == 0 && number.is_zero() {
                true => zeros += 1,
                false => number.mul_add(base, digit as u64),
            }
        }
        let mut bytes = vec![0; zeros];
        bytes.extend(number.to_be_bytes());
        Ok(bytes)
    }
}
