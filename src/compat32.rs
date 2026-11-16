//! 32-bit build only (i686-pc-windows-msvc).
//!
//! The aws-lc crypto library, when built for 32 bit, wrongly assumes that the
//! Microsoft compiler has the GCC builtins `__builtin_bswap16/32/64` and
//! references them - the linker cannot find them. We provide simple implementations
//! (byte swaps); the 64-bit build is not affected.

#[no_mangle]
pub extern "C" fn __builtin_bswap16(x: u16) -> u16 {
    x.swap_bytes()
}

#[no_mangle]
pub extern "C" fn __builtin_bswap32(x: u32) -> u32 {
    x.swap_bytes()
}

#[no_mangle]
pub extern "C" fn __builtin_bswap64(x: u64) -> u64 {
    x.swap_bytes()
}
