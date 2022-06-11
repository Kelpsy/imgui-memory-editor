#![allow(clippy::suspicious_arithmetic_impl)]

use core::ops::{Add, Div, Mul, Rem, Sub};

pub type YPosRaw = u128;
pub type SignedYPosRaw = i128;

const FRACT_BITS: u32 = 8;

macro_rules! impl_bin_ops {
    ($ty: ty, $raw: ty) => {
        impl_bin_ops!(
            $ty, $raw;
            Add, add, wrapped;
            Sub, sub, wrapped;
            Mul, mul, wrapped, shifted_res;
            Div, div, wrapped, shifted_lhs;
            Rem, rem, wrapped;
            Add, add, raw, shifted_rhs;
            Sub, sub, raw, shifted_rhs;
            Mul, mul, raw;
            Div, div, raw;
            Rem, rem, raw, shifted_rhs;
            Add, add, float f32;
            Sub, sub, float f32;
            Mul, mul, float f32;
            Div, div, float f32;
            Rem, rem, float f32;
            Add, add, float f64;
            Sub, sub, float f64;
            Mul, mul, float f64;
            Div, div, float f64;
            Rem, rem, float f64;
        );
    };

    ($ty: ty, $raw: ty;) => {};

    ($ty: ty, $raw: ty; $trait: ident, $fn: ident, wrapped; $($remaining: tt)*) => {
        impl $trait for $ty {
            type Output = Self;
            #[inline]
            fn $fn(self, rhs: Self) -> Self::Output {
                Self(self.0.$fn(rhs.0))
            }
        }
        impl_bin_ops!($ty, $raw; $($remaining)*);
    };

    ($ty: ty, $raw: ty; $trait: ident, $fn: ident, wrapped, shifted_lhs; $($remaining: tt)*) => {
        impl $trait for $ty {
            type Output = Self;
            #[inline]
            fn $fn(self, rhs: Self) -> Self::Output {
                Self((self.0 << FRACT_BITS).$fn(rhs.0))
            }
        }
        impl_bin_ops!($ty, $raw; $($remaining)*);
    };

    ($ty: ty, $raw: ty; $trait: ident, $fn: ident, wrapped, shifted_res; $($remaining: tt)*) => {
        impl $trait for $ty {
            type Output = Self;
            #[inline]
            fn $fn(self, rhs: Self) -> Self::Output {
                Self(self.0.$fn(rhs.0) >> FRACT_BITS)
            }
        }
        impl_bin_ops!($ty, $raw; $($remaining)*);
    };

    ($ty: ty, $raw: ty; $trait: ident, $fn: ident, raw; $($remaining: tt)*) => {
        impl $trait<$raw> for $ty {
            type Output = Self;
            #[inline]
            fn $fn(self, rhs: $raw) -> Self::Output {
                Self(self.0.$fn(rhs))
            }
        }
        impl_bin_ops!($ty, $raw; $($remaining)*);
    };

    ($ty: ty, $raw: ty; $trait: ident, $fn: ident, raw, shifted_rhs; $($remaining: tt)*) => {
        impl $trait<$raw> for $ty {
            type Output = Self;
            #[inline]
            fn $fn(self, rhs: $raw) -> Self::Output {
                Self(self.0.$fn(rhs << FRACT_BITS))
            }
        }
        impl_bin_ops!($ty, $raw; $($remaining)*);
    };

    ($ty: ty, $raw: ty; $trait: ident, $fn: ident, float $float_ty: ty; $($remaining: tt)*) => {
        impl $trait<$float_ty> for $ty {
            type Output = Self;
            #[inline]
            fn $fn(self, rhs: $float_ty) -> Self::Output {
                self.$fn(Self::from(rhs))
            }
        }
        impl_bin_ops!($ty, $raw; $($remaining)*);
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct YPos(pub YPosRaw);

impl YPos {
    #[inline]
    pub fn as_signed(self) -> SignedYPos {
        SignedYPos(self.0 as SignedYPosRaw)
    }

    #[inline]
    pub fn trunc(self) -> YPosRaw {
        self.0 >> FRACT_BITS
    }

    #[inline]
    pub fn fract(self) -> YPosRaw {
        self.0 & ((1 << FRACT_BITS) - 1)
    }

    #[inline]
    pub fn from_int(value: YPosRaw) -> Self {
        YPos(value << FRACT_BITS)
    }

    #[inline]
    pub fn saturating_add(self, other: Self) -> Self {
        YPos(self.0.saturating_add(other.0))
    }

    #[inline]
    pub fn saturating_sub(self, other: Self) -> Self {
        YPos(self.0.saturating_sub(other.0))
    }

    #[inline]
    pub fn div_into_int(self, other: Self) -> YPosRaw {
        self.0 / other.0
    }
}

impl From<f32> for YPos {
    #[inline]
    fn from(value: f32) -> Self {
        YPos((value as f64 * (1 << FRACT_BITS) as f64) as YPosRaw)
    }
}

impl From<YPos> for f32 {
    #[inline]
    fn from(value: YPos) -> Self {
        ((value.0 as f64) / (1 << FRACT_BITS) as f64) as f32
    }
}

impl From<f64> for YPos {
    #[inline]
    fn from(value: f64) -> Self {
        YPos((value * (1 << FRACT_BITS) as f64) as YPosRaw)
    }
}

impl From<YPos> for f64 {
    #[inline]
    fn from(value: YPos) -> Self {
        (value.0 as f64) / (1 << FRACT_BITS) as f64
    }
}

impl_bin_ops!(YPos, YPosRaw);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SignedYPos(pub SignedYPosRaw);

impl SignedYPos {
    #[inline]
    pub fn as_unsigned(self) -> YPos {
        YPos(self.0 as YPosRaw)
    }

    #[inline]
    pub fn trunc(self) -> SignedYPosRaw {
        self.0 / (1 << FRACT_BITS)
    }

    #[inline]
    pub fn fract(self) -> SignedYPosRaw {
        self.0 % (1 << FRACT_BITS)
    }

    #[inline]
    pub fn from_int(value: SignedYPosRaw) -> Self {
        SignedYPos(value << FRACT_BITS)
    }

    #[inline]
    pub fn div_int(self, other: Self) -> SignedYPosRaw {
        self.0 / other.0
    }
}

impl From<SignedYPosRaw> for SignedYPos {
    #[inline]
    fn from(int: SignedYPosRaw) -> Self {
        SignedYPos(int << FRACT_BITS)
    }
}

impl From<SignedYPos> for SignedYPosRaw {
    #[inline]
    fn from(value: SignedYPos) -> Self {
        value.0 >> FRACT_BITS
    }
}

impl From<f32> for SignedYPos {
    #[inline]
    fn from(value: f32) -> Self {
        SignedYPos((value as f64 * (1 << FRACT_BITS) as f64) as SignedYPosRaw)
    }
}

impl From<SignedYPos> for f32 {
    #[inline]
    fn from(value: SignedYPos) -> Self {
        ((value.0 as f64) / (1 << FRACT_BITS) as f64) as f32
    }
}

impl From<f64> for SignedYPos {
    #[inline]
    fn from(value: f64) -> Self {
        SignedYPos((value * (1 << FRACT_BITS) as f64) as SignedYPosRaw)
    }
}

impl From<SignedYPos> for f64 {
    #[inline]
    fn from(value: SignedYPos) -> Self {
        (value.0 as f64) / (1 << FRACT_BITS) as f64
    }
}

impl_bin_ops!(SignedYPos, SignedYPosRaw);
