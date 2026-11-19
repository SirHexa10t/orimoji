//! Just enough of a big natural number for what reads a whole run of digits as one number: packing,
//! word records, base58 and base94. It can multiply by a small number and add one, divide by a
//! small number, and give its bits. Limbs are `u64`, the least significant first, and never end in
//! a zero limb, so zero is no limbs at all.

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Nat {
    limbs: Vec<u64>,
}

impl Nat {
    pub(crate) fn zero() -> Self {
        Self::default()
    }

    pub(crate) fn one() -> Self {
        Self { limbs: vec![1] }
    }

    /// `self * factor + addend`, in place.
    pub(crate) fn mul_add(&mut self, factor: u64, addend: u64) {
        let mut carry = addend as u128;
        for limb in &mut self.limbs {
            let wide = *limb as u128 * factor as u128 + carry;
            *limb = wide as u64;
            carry = wide >> 64;
        }
        if carry > 0 {
            self.limbs.push(carry as u64);
        }
        self.trim();
    }

    /// Divides by `divisor` in place and gives the remainder.
    pub(crate) fn div_rem(&mut self, divisor: u64) -> u64 {
        let mut remainder = 0u128;
        for limb in self.limbs.iter_mut().rev() {
            let wide = (remainder << 64) | *limb as u128;
            *limb = (wide / divisor as u128) as u64;
            remainder = wide % divisor as u128;
        }
        self.trim();
        remainder as u64
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    pub(crate) fn is_one(&self) -> bool {
        self.limbs == [1]
    }

    /// Whether it is less than `bound`.
    pub(crate) fn below(&self, bound: u64) -> bool {
        match self.limbs[..] {
            [] => bound > 0,
            [only] => only < bound,
            _ => false,
        }
    }

    /// How many bits it takes: 0 for zero.
    pub(crate) fn bit_len(&self) -> usize {
        match self.limbs.last() {
            None => 0,
            Some(top) => (self.limbs.len() - 1) * 64 + (64 - top.leading_zeros() as usize),
        }
    }

    /// Bit `index`, counting from the least significant.
    pub(crate) fn bit(&self, index: usize) -> bool {
        self.limbs.get(index / 64).is_some_and(|limb| (limb >> (index % 64)) & 1 == 1)
    }

    /// Sets bit `index` to one.
    pub(crate) fn set_bit(&mut self, index: usize) {
        if self.limbs.len() <= index / 64 {
            self.limbs.resize(index / 64 + 1, 0);
        }
        self.limbs[index / 64] |= 1 << (index % 64);
    }

    /// `base` to the power `exponent`.
    pub(crate) fn power(base: u64, exponent: usize) -> Self {
        let mut power = Self::one();
        for _ in 0..exponent {
            power.mul_add(base, 0);
        }
        power
    }

    /// One less than itself; zero stays zero.
    pub(crate) fn decrement(&mut self) {
        for limb in &mut self.limbs {
            let (value, borrow) = limb.overflowing_sub(1);
            *limb = value;
            if !borrow {
                break;
            }
        }
        self.trim();
    }

    /// The number these big-endian bytes spell.
    pub(crate) fn from_be_bytes(bytes: &[u8]) -> Self {
        let mut number = Self::zero();
        for &byte in bytes {
            number.mul_add(256, byte as u64);
        }
        number
    }

    /// Its big-endian bytes, as few as it needs: none for zero.
    pub(crate) fn to_be_bytes(&self) -> Vec<u8> {
        let mut bytes: Vec<u8> =
            self.limbs.iter().rev().flat_map(|limb| limb.to_be_bytes()).collect();
        let leading = bytes.iter().take_while(|&&byte| byte == 0).count();
        bytes.drain(..leading);
        bytes
    }

    fn trim(&mut self) {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_written_in_go_back_out_in_reverse() {
        let digits = [7u64, 0, 93, 12, 0, 0, 55, 1];
        let mut number = Nat::zero();
        for &digit in &digits {
            number.mul_add(94, digit);
        }
        let mut back = Vec::new();
        while !number.is_zero() {
            back.push(number.div_rem(94));
        }
        back.reverse();
        assert_eq!(back, digits);
    }

    #[test]
    fn bytes_and_bits_agree_with_the_number() {
        let number = Nat::from_be_bytes(&[0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 2]);
        assert_eq!(number.to_be_bytes(), [1, 0, 0, 0, 0, 0, 0, 0, 0, 2], "leading zeros dropped");
        assert_eq!(number.bit_len(), 73);
        assert!(number.bit(1) && number.bit(72) && !number.bit(0) && !number.bit(500));
        let mut built = Nat::zero();
        built.set_bit(72);
        built.set_bit(1);
        assert_eq!(built, number);
    }

    #[test]
    fn powers_and_decrements_meet_at_all_ones() {
        let mut number = Nat::power(2, 130);
        number.decrement();
        assert_eq!(number.bit_len(), 130);
        assert!((0..130).all(|index| number.bit(index)));
        let mut zero = Nat::zero();
        zero.decrement();
        assert!(zero.is_zero());
    }
}
