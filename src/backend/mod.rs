use crate::{Error, Input, Output, Result, processor::Control, vad::Vad};
use serde::Serialize;
use std::{
    ffi::{CStr, CString, c_char, c_void},
    path::Path,
    ptr::NonNull,
};

#[derive(Clone, Debug, Serialize)]
pub struct AudioInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub stream: u32,
    pub frames: u64,
    pub bitrate: Option<u64>,
    pub bits_per_sample: Option<u16>,
    pub codec: String,
    pub sample_format: String,
}
#[repr(C)]
struct Info {
    rate: i32,
    channels: i32,
    stream: i32,
    frames: i64,
    bitrate: i64,
    bits_per_sample: i32,
    codec: [c_char; 64],
    sample_format: [c_char; 32],
}
impl Default for Info {
    fn default() -> Self {
        Self {
            rate: 0,
            channels: 0,
            stream: 0,
            frames: 0,
            bitrate: 0,
            bits_per_sample: 0,
            codec: [0; 64],
            sample_format: [0; 32],
        }
    }
}
impl Info {
    fn owned(&self) -> AudioInfo {
        AudioInfo {
            sample_rate: self.rate as u32,
            channels: self.channels as u16,
            stream: self.stream as u32,
            frames: self.frames as u64,
            bitrate: (self.bitrate > 0).then_some(self.bitrate as u64),
            bits_per_sample: (self.bits_per_sample > 0).then_some(self.bits_per_sample as u16),
            codec: unsafe { CStr::from_ptr(self.codec.as_ptr()) }
                .to_string_lossy()
                .into_owned(),
            sample_format: unsafe { CStr::from_ptr(self.sample_format.as_ptr()) }
                .to_string_lossy()
                .into_owned(),
        }
    }
}
type Cancel = extern "C" fn(*mut c_void) -> i32;
type Samples = extern "C" fn(*mut c_void, *const i16, i32) -> i32;
unsafe extern "C" {
    fn sa_logging_init();
    fn sa_logging_scope(sink: *mut c_char) -> *mut c_char;
    fn sa_open(
        path: *const c_char,
        raw: *const c_char,
        raw_rate: i32,
        raw_channels: i32,
        stream: i32,
        cancel: Cancel,
        context: *mut c_void,
        info: *mut Info,
        error: *mut c_char,
    ) -> *mut c_void;
    fn sa_next(reader: *mut c_void, start: *mut i64, count: *mut i32, error: *mut c_char) -> i32;
    fn sa_start_skip(reader: *mut c_void) -> i64;
    fn sa_encoder_delay(writer: *mut c_void) -> i32;
    fn sa_analyze(
        reader: *mut c_void,
        samples: Samples,
        context: *mut c_void,
        frames: *mut i64,
        error: *mut c_char,
    ) -> i32;
    fn sa_reader_free(reader: *mut c_void);
    fn sa_writer_open(
        reader: *mut c_void,
        path: *const c_char,
        muxer: *const c_char,
        codec: *const c_char,
        rate: i32,
        channels: i32,
        bitrate: i64,
        format: *const c_char,
        threads: i32,
        error: *mut c_char,
    ) -> *mut c_void;
    fn sa_write(
        writer: *mut c_void,
        reader: *mut c_void,
        offset: i32,
        count: i32,
        error: *mut c_char,
    ) -> i32;
    fn sa_finish(writer: *mut c_void, info: *mut Info, error: *mut c_char) -> i32;
    fn sa_writer_free(writer: *mut c_void);
    fn sa_supported(muxer: *const c_char, codec: *const c_char) -> i32;
    fn sa_version() -> *const c_char;
}
struct LogScope {
    _buffer: Box<[c_char; 1024]>,
    previous: *mut c_char,
}
impl LogScope {
    fn new() -> Self {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| unsafe { sa_logging_init() });
        let mut buffer = Box::new([0; 1024]);
        let previous = unsafe { sa_logging_scope(buffer.as_mut_ptr()) };
        Self {
            _buffer: buffer,
            previous,
        }
    }
}
impl Drop for LogScope {
    fn drop(&mut self) {
        unsafe {
            sa_logging_scope(self.previous);
        }
    }
}
fn string(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| crate::error::invalid("NUL in string"))
}
fn path(p: &Path) -> Result<CString> {
    string(
        p.to_str()
            .ok_or_else(|| crate::error::invalid("paths must be valid UTF-8"))?,
    )
}
fn message(e: &[c_char]) -> String {
    unsafe { CStr::from_ptr(e.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}
extern "C" fn cancelled(context: *mut c_void) -> i32 {
    i32::from(
        unsafe { &*context.cast::<Control<'_>>() }
            .cancellation
            .is_cancelled(),
    )
}
pub(crate) struct Reader<'a> {
    ptr: NonNull<c_void>,
    pub info: AudioInfo,
    control: &'a Control<'a>,
}
impl<'a> Reader<'a> {
    pub fn start_skip(&self) -> u64 {
        unsafe { sa_start_skip(self.ptr.as_ptr()).max(0) as u64 }
    }
    pub fn open(input: &Input, control: &'a Control<'a>) -> Result<Self> {
        let _logs = LogScope::new();
        control.check()?;
        if version() != "8.1.2" {
            return Err(crate::error::invalid(format!(
                "FFmpeg 8.1.2 is required; linked {}",
                version()
            )));
        }
        if let Some(raw) = &input.raw
            && (!crate::formats::raw(&raw.format)
                || raw.sample_rate == 0
                || raw.sample_rate > 768000
                || raw.channels == 0
                || raw.channels > 64)
        {
            return Err(crate::error::invalid(
                "raw input requires a registered format and valid rate/channels",
            ));
        }
        let p = path(&input.path)?;
        let raw = string(input.raw.as_ref().map_or("", |r| r.format.as_str()))?;
        let stream = input
            .stream
            .map(i32::try_from)
            .transpose()
            .map_err(|_| crate::error::invalid("stream index overflow"))?
            .unwrap_or(-1);
        let mut info = Info::default();
        let mut e = [0; 1024];
        let ptr = unsafe {
            sa_open(
                p.as_ptr(),
                raw.as_ptr(),
                input.raw.as_ref().map_or(0, |r| r.sample_rate as i32),
                input.raw.as_ref().map_or(0, |r| i32::from(r.channels)),
                stream,
                cancelled,
                (control as *const Control<'_>).cast_mut().cast(),
                &mut info,
                e.as_mut_ptr(),
            )
        };
        if let Err(error) = control.check() {
            unsafe { sa_reader_free(ptr) };
            return Err(error);
        }
        Ok(Self {
            ptr: NonNull::new(ptr).ok_or_else(|| Error::Conversion(message(&e)))?,
            info: info.owned(),
            control,
        })
    }
    pub fn next(&mut self) -> Result<Option<(u64, u64)>> {
        let _logs = LogScope::new();
        let mut start = 0;
        let mut count = 0;
        let mut e = [0; 1024];
        let ret = unsafe { sa_next(self.ptr.as_ptr(), &mut start, &mut count, e.as_mut_ptr()) };
        self.control.check()?;
        if ret < 0 {
            Err(Error::Conversion(message(&e)))
        } else if ret == 0 {
            Ok(None)
        } else {
            self.info.frames = start as u64 + count as u64;
            Ok(Some((start as u64, count as u64)))
        }
    }
    pub fn analyze(&mut self, start_threshold: f32) -> Result<Vec<crate::Interval>> {
        let _logs = LogScope::new();
        struct State<'a> {
            vad: Vad,
            error: Option<Error>,
            control: &'a Control<'a>,
            samples: u64,
            source_rate: u32,
        }
        extern "C" fn samples(context: *mut c_void, data: *const i16, count: i32) -> i32 {
            let state = unsafe { &mut *context.cast::<State>() };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                state
                    .vad
                    .push(unsafe { std::slice::from_raw_parts(data, count as usize) })?;
                let old = state.samples;
                state.samples += count as u64;
                if old / 16000 != state.samples / 16000 {
                    state.control.emit(crate::Event::Decoding {
                        source_frames: crate::mapping::scale(
                            state.samples,
                            state.source_rate,
                            16000,
                            false,
                        )?,
                    })?;
                }
                Ok(())
            }));
            match result {
                Ok(Ok(())) => 0,
                Ok(Err(e)) => {
                    state.error = Some(e);
                    1
                }
                Err(_) => {
                    state.error = Some(Error::Detection("Earshot callback panicked".into()));
                    1
                }
            }
        }
        let mut state = State {
            vad: Vad::new(start_threshold),
            error: None,
            control: self.control,
            samples: 0,
            source_rate: self.info.sample_rate,
        };
        let mut e = [0; 1024];
        let mut frames = 0;
        let ret = unsafe {
            sa_analyze(
                self.ptr.as_ptr(),
                samples,
                (&mut state as *mut State).cast(),
                &mut frames,
                e.as_mut_ptr(),
            )
        };
        self.control.check()?;
        if let Some(e) = state.error {
            return Err(e);
        }
        if ret < 0 {
            return Err(Error::Detection(message(&e)));
        }
        self.info.frames = frames as u64;
        self.control.emit(crate::Event::Diagnostic {
            message: state.vad.diagnostic(),
        })?;
        state.vad.finish()
    }
}
impl Drop for Reader<'_> {
    fn drop(&mut self) {
        let _logs = LogScope::new();
        unsafe { sa_reader_free(self.ptr.as_ptr()) }
    }
}
pub(crate) struct Writer {
    ptr: NonNull<c_void>,
    cancellation: crate::Cancellation,
}
impl Writer {
    pub fn open(reader: &Reader<'_>, p: &Path, o: &Output) -> Result<Self> {
        let _logs = LogScope::new();
        let p = path(p)?;
        let mux = string(crate::formats::container(&o.container)?.muxer)?;
        let codec = string(&o.encoder)?;
        let fmt = string(o.sample_format.as_deref().unwrap_or(""))?;
        let mut e = [0; 1024];
        let ptr = unsafe {
            sa_writer_open(
                reader.ptr.as_ptr(),
                p.as_ptr(),
                mux.as_ptr(),
                codec.as_ptr(),
                o.sample_rate as i32,
                i32::from(o.channels),
                i64::from(o.bitrate.unwrap_or(0)),
                fmt.as_ptr(),
                i32::from(o.threads),
                e.as_mut_ptr(),
            )
        };
        let Some(ptr) = NonNull::new(ptr) else {
            reader.control.check()?;
            return Err(Error::Conversion(message(&e)));
        };
        let writer = Self {
            ptr,
            cancellation: reader.control.cancellation.clone(),
        };
        reader.control.check()?;
        Ok(writer)
    }
    pub fn write(&mut self, r: &Reader<'_>, offset: u64, count: u64) -> Result<()> {
        let _logs = LogScope::new();
        let mut e = [0; 1024];
        let ret = unsafe {
            sa_write(
                self.ptr.as_ptr(),
                r.ptr.as_ptr(),
                offset as i32,
                count as i32,
                e.as_mut_ptr(),
            )
        };
        r.control.check()?;
        if ret < 0 {
            Err(Error::Conversion(message(&e)))
        } else {
            Ok(())
        }
    }
    pub fn finish(mut self) -> Result<(AudioInfo, u64)> {
        let _logs = LogScope::new();
        let mut info = Info::default();
        let mut e = [0; 1024];
        let ret = unsafe { sa_finish(self.ptr.as_mut(), &mut info, e.as_mut_ptr()) };
        if self.cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if ret < 0 {
            Err(Error::Conversion(message(&e)))
        } else {
            Ok((info.owned(), unsafe {
                sa_encoder_delay(self.ptr.as_ptr()).max(0) as u64
            }))
        }
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        let _logs = LogScope::new();
        unsafe { sa_writer_free(self.ptr.as_ptr()) }
    }
}
pub fn available(container: &str, codec: &str) -> bool {
    let Ok(c) = crate::formats::container(container) else {
        return false;
    };
    let (Ok(m), Ok(e)) = (string(c.muxer), string(codec)) else {
        return false;
    };
    crate::formats::accepts(container, codec)
        && unsafe { sa_supported(m.as_ptr(), e.as_ptr()) != 0 }
}
pub fn version() -> String {
    unsafe { CStr::from_ptr(sa_version()) }
        .to_string_lossy()
        .into_owned()
}
