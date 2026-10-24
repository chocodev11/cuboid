//! Exact decimal digits of an f32 and the rounding stage of the number text formatters.

/// Capacity for the longest exact expansion: an odd 24-bit mantissa times `5^149` has at most
/// 112 digits, and a chunk of the conversion writes 9 digits at a time.
const CAPACITY: usize = 120;

/// The significant decimal digits of a finite magnitude: `digits[0].digits[1]... * 10^exp`.
/// `digits[0]` is not 0 and the last kept digit is not 0; `len == 0` is the value zero.
#[derive(Clone, Debug)]
pub(super) struct Digits {
    pub digits: [u8; CAPACITY],
    pub len: usize,
    pub exp: i32,
}

impl Digits {
    /// The exact digits of the binary value of `x`, which must be finite and non-zero. The sign
    /// is ignored.
    pub fn exact(x: f32) -> Self {
        let bits = x.to_bits() & 0x7FFF_FFFF;
        let fraction = bits & 0x7F_FFFF;
        let biased = (bits >> 23) as i32;
        // value = mantissa * 2^exp2
        let (mut mantissa, mut exp2) = if biased == 0 {
            (fraction, -149)
        } else {
            (fraction | 0x80_0000, biased - 150)
        };
        let zeros = mantissa.trailing_zeros();
        mantissa >>= zeros;
        exp2 += zeros as i32;

        // An integer N and a decimal point `shift` places from its right end: for a negative
        // exponent mantissa / 2^k = mantissa * 5^k / 10^k.
        let mut n = Big::new(mantissa);
        let shift = if exp2 >= 0 {
            n.mul_pow(1 << 31, 2, exp2.unsigned_abs());
            0
        } else {
            n.mul_pow(1_220_703_125, 5, exp2.unsigned_abs());
            exp2.unsigned_abs() as i32
        };

        let mut digits = [0u8; CAPACITY];
        let mut count = 0;
        while !n.is_zero() {
            let mut chunk = n.div_small(1_000_000_000);
            for _ in 0..9 {
                digits[count] = (chunk % 10) as u8;
                chunk /= 10;
                count += 1;
            }
        }
        while digits[count - 1] == 0 {
            count -= 1;
        }
        digits[..count].reverse();
        let exp = count as i32 - 1 - shift;
        let mut len = count;
        while digits[len - 1] == 0 {
            len -= 1;
        }
        Self { digits, len, exp }
    }

    /// Rounds to `keep` significant digits; the first dropped digit decides and 5 or more
    /// rounds up, which is the digit rounding of the runtime's number formatter. `keep` may be
    /// 0 (everything is dropped, the result is 0 or 1 at the next place) or negative (the
    /// result is 0).
    pub fn round(&mut self, keep: i32) {
        let Ok(keep) = usize::try_from(keep) else {
            self.len = 0;
            return;
        };
        if keep >= self.len {
            return;
        }
        let up = self.digits[keep] >= 5;
        self.len = keep;
        if !up {
            self.strip_zeros();
            return;
        }
        while self.len > 0 && self.digits[self.len - 1] == 9 {
            self.len -= 1;
        }
        if self.len == 0 {
            self.digits[0] = 1;
            self.len = 1;
            self.exp += 1;
        } else {
            self.digits[self.len - 1] += 1;
        }
    }

    fn strip_zeros(&mut self) {
        while self.len > 0 && self.digits[self.len - 1] == 0 {
            self.len -= 1;
        }
    }

    pub fn is_zero(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.digits[..self.len]
    }
}

/// An unsigned integer of up to 12 little-endian 32-bit limbs; `5^149` times a 24-bit mantissa
/// needs 370 bits.
struct Big {
    limbs: [u32; 12],
    len: usize,
}

impl Big {
    fn new(v: u32) -> Self {
        let mut limbs = [0; 12];
        limbs[0] = v;
        Self { limbs, len: 1 }
    }

    fn is_zero(&self) -> bool {
        self.len == 0
    }

    /// Multiplies by `base^count`, in steps of `step = base^k` that fit a limb.
    fn mul_pow(&mut self, step: u32, base: u32, count: u32) {
        let per_step = step.ilog(base);
        let mut left = count;
        while left >= per_step {
            self.mul_small(step);
            left -= per_step;
        }
        self.mul_small(base.pow(left));
    }

    fn mul_small(&mut self, factor: u32) {
        let mut carry = 0u64;
        for limb in &mut self.limbs[..self.len] {
            let t = u64::from(*limb) * u64::from(factor) + carry;
            *limb = t as u32;
            carry = t >> 32;
        }
        if carry != 0 {
            self.limbs[self.len] = carry as u32;
            self.len += 1;
        }
    }

    /// Divides in place and returns the remainder.
    fn div_small(&mut self, divisor: u32) -> u32 {
        let d = u64::from(divisor);
        let mut rem = 0u64;
        for limb in self.limbs[..self.len].iter_mut().rev() {
            let t = (rem << 32) | u64::from(*limb);
            *limb = (t / d) as u32;
            rem = t % d;
        }
        while self.len > 0 && self.limbs[self.len - 1] == 0 {
            self.len -= 1;
        }
        rem as u32
    }
}
