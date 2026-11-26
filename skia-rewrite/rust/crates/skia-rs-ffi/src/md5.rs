use skia_rs_math::md5::Md5State;

#[repr(C)]
pub struct SkiaRsMd5 {
    pub byte_count: u64,
    pub state: [u32; 4],
    pub buffer: [u8; 64],
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_md5_init(out: *mut SkiaRsMd5) {
    let s = Md5State::new();
    let dst = &mut *out;
    dst.byte_count = s.byte_count;
    dst.state = s.state;
    dst.buffer = s.buffer;
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_md5_update(
    raw: *mut SkiaRsMd5,
    data: *const u8,
    len: usize,
) {
    if len == 0 {
        return;
    }
    let dst = &mut *raw;
    let slice = core::slice::from_raw_parts(data, len);
    let mut s = Md5State {
        byte_count: dst.byte_count,
        state: dst.state,
        buffer: dst.buffer,
    };
    s.update(slice);
    dst.byte_count = s.byte_count;
    dst.state = s.state;
    dst.buffer = s.buffer;
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_md5_finish(raw: *mut SkiaRsMd5, digest_out: *mut [u8; 16]) {
    let dst = &mut *raw;
    let mut s = Md5State {
        byte_count: dst.byte_count,
        state: dst.state,
        buffer: dst.buffer,
    };
    let digest = s.finish();
    dst.byte_count = s.byte_count;
    dst.state = s.state;
    dst.buffer = s.buffer;
    *digest_out = digest;
}
