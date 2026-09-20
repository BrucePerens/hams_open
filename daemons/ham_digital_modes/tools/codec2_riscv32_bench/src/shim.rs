//! no_std stand-ins for the few std items the codec files use.
use core::cell::UnsafeCell;

pub struct OnceLock<T>(UnsafeCell<Option<T>>);
unsafe impl<T> Sync for OnceLock<T> {}
impl<T> OnceLock<T> {
    pub const fn new() -> Self { OnceLock(UnsafeCell::new(None)) }
    pub fn get_or_init<F: FnOnce() -> T>(&self, f: F) -> &T {
        unsafe {
            let p = &mut *self.0.get();
            if p.is_none() { *p = Some(f()); }
            p.as_ref().unwrap()
        }
    }
}

pub trait FloatMath: Sized {
    fn sqrt(self) -> Self; fn sin(self) -> Self; fn cos(self) -> Self; fn powi(self, n: i32) -> Self;
    fn round(self) -> Self; fn floor(self) -> Self; fn exp2(self) -> Self; fn log2(self) -> Self;
    fn sin_cos(self) -> (Self, Self); fn acos(self) -> Self; fn exp(self) -> Self; fn ln(self) -> Self;
    fn powf(self, e: Self) -> Self;
}
impl FloatMath for f32 {
    fn sqrt(self) -> f32 { libm::sqrtf(self) } fn sin(self) -> f32 { libm::sinf(self) }
    fn cos(self) -> f32 { libm::cosf(self) } fn powi(self, n: i32) -> f32 { libm::powf(self, n as f32) }
    fn round(self) -> f32 { libm::roundf(self) } fn floor(self) -> f32 { libm::floorf(self) }
    fn exp2(self) -> f32 { libm::exp2f(self) } fn log2(self) -> f32 { libm::log2f(self) }
    fn sin_cos(self) -> (f32, f32) { libm::sincosf(self) } fn acos(self) -> f32 { libm::acosf(self) }
    fn exp(self) -> f32 { libm::expf(self) } fn ln(self) -> f32 { libm::logf(self) }
    fn powf(self, e: f32) -> f32 { libm::powf(self, e) }
}
impl FloatMath for f64 {
    fn sqrt(self) -> f64 { libm::sqrt(self) } fn sin(self) -> f64 { libm::sin(self) }
    fn cos(self) -> f64 { libm::cos(self) } fn powi(self, n: i32) -> f64 { libm::pow(self, n as f64) }
    fn round(self) -> f64 { libm::round(self) } fn floor(self) -> f64 { libm::floor(self) }
    fn exp2(self) -> f64 { libm::exp2(self) } fn log2(self) -> f64 { libm::log2(self) }
    fn sin_cos(self) -> (f64, f64) { libm::sincos(self) } fn acos(self) -> f64 { libm::acos(self) }
    fn exp(self) -> f64 { libm::exp(self) } fn ln(self) -> f64 { libm::log(self) }
    fn powf(self, e: f64) -> f64 { libm::pow(self, e) }
}

