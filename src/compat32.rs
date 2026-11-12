//! Только для 32-битной сборки (i686-pc-windows-msvc).
//!
//! Библиотека шифрования aws-lc при сборке под 32 бит ошибочно решает, что у
//! компилятора Microsoft есть встроенные функции GCC `__builtin_bswap16/32/64`, и
//! ссылается на них - компоновщик их не находит. Даем простые реализации
//! (перестановка байт); на 64-битную сборку это не влияет.

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
