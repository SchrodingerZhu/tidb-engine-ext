#[cfg(feature = "symbolization")]
mod object;
#[cfg(feature = "symbolization")]
use object::{locate_symbol_in_object, LocationInObject};
#[cfg(feature = "symbolization")]
use std::os::raw::c_void;
use std::os::raw::{c_char, c_int};

/// # Safety
/// Print version information to std output.
#[no_mangle]
pub unsafe extern "C" fn print_raftstore_proxy_version() {
    server::print_proxy_version();
}

/// # Safety
/// Please make sure such function will be run in an independent thread. Usage about interfaces can be found in `struct EngineStoreServerHelper`.
#[no_mangle]
pub unsafe extern "C" fn run_raftstore_proxy_ffi(
    argc: c_int,
    argv: *const *const c_char,
    helper: *const u8,
) {
    server::run_proxy(argc, argv, helper);
}

/// Address Symbolization
/// Refer [this document](https://docs.rs/findshlibs/latest/findshlibs/#addresses) for explanations
/// of what is AVMA, SVMA, BIAS.
/// ------------------------------------------------------------------------------------------------
/// We pass raw pointers in `SymbolInfo`.
/// The assumption is that `backtrace` will resolve symbols to image maps (mmap of binary files).
/// These maps have static life-time.
#[cfg(feature = "symbolization")]
#[repr(C)]
pub struct SymbolInfo {
    /// corresponding function name
    pub symbol_name: *const c_char,
    /// binary file containing the symbol
    pub object_name: *const c_char,
    /// filename of the source.
    pub source_filename: *const c_char,
    /// length of source filename.
    /// this is needed because source filename may not have terminate nul.
    pub source_filename_length: usize,
    /// source line number.
    pub lineno: usize,
    /// symbol offset to the beginning of the binary file.
    pub svma: usize,
}

#[cfg(feature = "symbolization")]
impl Default for SymbolInfo {
    fn default() -> Self {
        SymbolInfo {
            symbol_name: std::ptr::null(),
            object_name: std::ptr::null(),
            source_filename: std::ptr::null(),
            source_filename_length: 0,
            lineno: 0,
            svma: 0,
        }
    }
}

#[cfg(feature = "symbolization")]
impl SymbolInfo {
    pub fn new(avma: *mut c_void) -> Self {
        let mut info = SymbolInfo::default();
        if let Some(LocationInObject { name, svma }) = locate_symbol_in_object(avma) {
            info.object_name = name;
            info.svma = svma;
        }
        backtrace::resolve(avma, |sym| {
            info.symbol_name = sym
                .name()
                .map(|x| x.as_bytes().as_ptr() as *const _)
                .unwrap_or_else(std::ptr::null);
            if let Some(src) = sym.filename().and_then(|x| x.to_str()) {
                info.source_filename = src.as_ptr() as _;
                info.source_filename_length = src.len();
            }
            info.lineno = sym.lineno().map(|x| x as _).unwrap_or(0)
        });
        info
    }
}

/// # Safety
/// `_tiflash_symbolize` is thread-safe. However, one should avoid reentrancy.
///
/// # Note
/// `_tiflash_symbolize` starts with `_`, indicating that this function is designed for internal
/// usage. Make sure you understand the mechanism behind this function before put it into your code!

#[cfg(feature = "symbolization")]
#[no_mangle]
pub unsafe extern "C" fn _tiflash_symbolize(avma: *mut c_void) -> SymbolInfo {
    SymbolInfo::new(avma)
}
