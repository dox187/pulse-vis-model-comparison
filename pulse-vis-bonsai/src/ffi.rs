
use std::ffi::CString;
use std::os::raw::c_void;
use std::str;

#[allow(dead_code)]
pub(crate) mod deps {
    use std::os::raw::{c_int, c_void};

    #[repr(C)]
    #[derive(Clone)]
    pub struct PVzSourceInfo {
        pub name: [u8; 512],
        pub alias: [u8; 256],
        pub source: u32,
        pub channels: i32,
        pub rate: i32,
        pub sample_format: i32,
        pub client: u32,
        pub index: u32,
        pub mute: i32,
    }

    unsafe extern "C" {
        pub(crate) fn pulse_viz_mainloop_new() -> *mut c_void;
        pub(crate) fn pulse_viz_mainloop_free(_m: *mut c_void);
        pub(crate) fn pulse_viz_mainloop_iterate(_m: *mut c_void, _block: c_int, _retval: *mut c_int) -> i32;
        pub(crate) fn pulse_viz_context_new(_ml: *mut c_void) -> *mut c_void;
        pub(crate) fn pulse_viz_context_free(_c: *mut c_void);
        pub(crate) fn pulse_viz_context_connect(_c: *mut c_void) -> i32;
        pub(crate) fn pulse_viz_context_errno(_c: *mut c_void) -> i32;
        pub(crate) fn pulse_viz_context_state(_c: *mut c_void) -> i32;
        pub(crate) fn pulse_viz_stream_new(_c: *mut c_void, source_name: *const u8, rate: c_int, channels: c_int, sample_format: c_int) -> *mut c_void;
        pub(crate) fn pulse_viz_stream_free(_s: *mut c_void);
        pub(crate) fn pulse_viz_stream_get_state(_s: *mut c_void) -> i32;
        pub(crate) fn pulse_viz_stream_get_rate(_s: *mut c_void) -> i32;
        pub(crate) fn pulse_viz_stream_get_channels(_s: *mut c_void) -> i32;
        pub(crate) fn pulse_viz_stream_get_sample_format(_s: *mut c_void) -> i32;
        pub(crate) fn pulse_viz_stream_readable_size(_s: *mut c_void) -> usize;
        pub(crate) fn pulse_viz_stream_read(_s: *mut c_void, _buf: *mut c_void, _size: usize) -> usize;
        pub(crate) fn pulse_viz_stream_set_read_callback(_s: *mut c_void);
        pub(crate) fn pulse_viz_stream_set_state_callback(_s: *mut c_void);
        pub(crate) fn pulse_viz_start_source_enum(_c: *mut c_void) -> *mut c_void;
        pub(crate) fn pulse_viz_get_src_op_state(_op: *mut c_void) -> i32;
        pub(crate) fn pulse_viz_get_src_op_list(_op: *mut c_void, _count_out: *mut c_int) -> *const PVzSourceInfo;
        pub(crate) fn pulse_viz_free_src_op(_op: *mut c_void);
    }
}

#[derive(Clone, Debug)]
    pub struct SourceInfo {
    pub name: String,
    pub alias: String,
    pub source: u32,
    pub channels: i32,
    pub rate: i32,
    pub sample_format: i32,
    pub client: u32,
    pub index: u32,
    pub mute: i32,
}

pub struct Mainloop(pub *mut c_void);
pub struct Context(pub *mut c_void);
pub struct Stream(pub *mut c_void);
pub struct SrcOp(pub *mut c_void);

impl Mainloop {
    pub fn new() -> Self {
        Self(unsafe { deps::pulse_viz_mainloop_new() })
    }

    pub fn iterate(&self, block: bool) -> i32 {
        let mut rv: i32 = 0;
        unsafe { deps::pulse_viz_mainloop_iterate(self.0, if block { 1 } else { 0 }, &mut rv) }
    }

    pub fn stop(&self) {
        if !self.0.is_null() {
            unsafe { deps::pulse_viz_mainloop_free(self.0) }
        }
    }
}

impl Drop for Mainloop {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Context {
    pub fn new(mainloop: &Mainloop) -> Self {
        Self(unsafe { deps::pulse_viz_context_new(mainloop.0) })
    }
    pub fn connect(&self) -> i32 {
        unsafe { deps::pulse_viz_context_connect(self.0) }
    }
    pub fn errno(&self) -> i32 {
        unsafe { deps::pulse_viz_context_errno(self.0) }
    }
    pub fn state(&self) -> i32 {
        unsafe { deps::pulse_viz_context_state(self.0) }
    }
    pub fn stream_new(&self, source_name: &str, rate: i32, channels: i32, sample_format: i32) -> Stream {
        let name: *const u8 = if source_name.is_empty() {
            std::ptr::null()
        } else {
            let c = CString::new(source_name).expect("source name invalid");
            c.into_raw() as *const u8
        };
        Stream(unsafe { deps::pulse_viz_stream_new(self.0, name, rate, channels, sample_format) })
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { deps::pulse_viz_context_free(self.0) }
        }
    }
}

impl Stream {
    pub fn get_state(&self) -> i32 {
        unsafe { deps::pulse_viz_stream_get_state(self.0) }
    }
    pub fn get_rate(&self) -> i32 {
        unsafe { deps::pulse_viz_stream_get_rate(self.0) }
    }
    pub fn get_channels(&self) -> i32 {
        unsafe { deps::pulse_viz_stream_get_channels(self.0) }
    }
    pub fn get_sample_format(&self) -> i32 {
        unsafe { deps::pulse_viz_stream_get_sample_format(self.0) }
    }
    pub fn readable_size(&self) -> usize {
        unsafe { deps::pulse_viz_stream_readable_size(self.0) }
    }
    pub fn read(&self, buf: &[u8]) -> usize {
        if buf.is_empty() { return 0; }
        unsafe { deps::pulse_viz_stream_read(self.0, buf.as_ptr() as *mut c_void, buf.len()) }
    }
    fn free(&self) {
        if !self.0.is_null() {
            unsafe { deps::pulse_viz_stream_free(self.0) }
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.free();
    }
}

impl SrcOp {
    pub fn new(op: *mut c_void) -> Self {
        Self(op)
    }
    pub fn get_state(&self) -> i32 {
        unsafe { deps::pulse_viz_get_src_op_state(self.0) }
    }
    fn free(&self) {
        if !self.0.is_null() {
            unsafe { deps::pulse_viz_free_src_op(self.0) }
        }
    }
}

impl Drop for SrcOp {
    fn drop(&mut self) {
        self.free();
    }
}

pub fn get_source_list(op: &SrcOp) -> Vec<SourceInfo> {
    let mut count: i32 = 0;
    let ptr = unsafe { deps::pulse_viz_get_src_op_list(op.0, &mut count) };
    if ptr.is_null() || count <= 0 {
        return vec![];
    }
    let count = count as usize;
    (0..count).map(|i| {
        let raw = unsafe { (*ptr.add(i)).clone() };
        let name_len = raw.name.iter().position(|&b| b == 0).unwrap_or(raw.name.len());
        let alias_len = raw.alias.iter().position(|&b| b == 0).unwrap_or(raw.alias.len());
        let name_str = std::str::from_utf8(&raw.name[0..name_len]).unwrap_or("unknown");
        let alias_str = std::str::from_utf8(&raw.alias[0..alias_len]).unwrap_or("");
        SourceInfo {
            name: name_str.to_string(),
            alias: alias_str.to_string(),
            source: raw.source,
            channels: raw.channels,
            rate: raw.rate,
            sample_format: raw.sample_format,
            client: raw.client,
            index: raw.index,
            mute: raw.mute,
        }
    }).collect()
}
