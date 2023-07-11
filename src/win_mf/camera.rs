use super::win_mf::{
    self, activate_to_media_source, capture_engine_prepare_sample_callback,
    capture_engine_sink_get_media_type, co_initialize_multithreaded, enum_device_sources,
    init_capture_engine, new_capture_engine, sample_to_locked_buffer, CameraFrame,
    CaptureEngineEvent, CaptureEventCallback, CaptureSampleCallback, Device,
};

use std::{ffi::OsString, mem::MaybeUninit, sync::mpsc::*};

use windows::{
    core::*,
    Win32::{Media::MediaFoundation::*, System::Com::*},
};

use super::attributes::{mf_create_attributes, mf_get_string};
use super::media_type::MediaType;

pub struct Camera {
    engine: IMFCaptureEngine,
    device: IMFActivate,
    media_source: IMFMediaSource,
    event_rx: Receiver<CaptureEngineEvent>,
    sample_rx: Receiver<Option<IMFSample>>,
    event_cb: IMFCaptureEngineOnEventCallback,
    sample_cb: IMFCaptureEngineOnSampleCallback,
}

#[derive(Debug)]
pub struct Frame {
    frame: win_mf::CameraFrame,
}

pub struct FrameData<'a> {
    data: &'a [u8],
}

impl Camera {
    pub fn new_default_device() -> Self {
        co_initialize_multithreaded();
        win_mf::media_foundation_startup().expect("media_foundation_startup");

        let engine = new_capture_engine().unwrap();
        let (event_tx, event_rx) = channel::<CaptureEngineEvent>();
        let (sample_tx, sample_rx) = channel::<Option<IMFSample>>();
        let event_cb = CaptureEventCallback { event_tx }.into();
        let sample_cb = CaptureSampleCallback { sample_tx }.into();

        let devices = enum_device_sources();
        let Some(device) = devices.first().cloned() else { todo!() };
        let media_source = activate_to_media_source(&device);

        init_capture_engine(&engine, Some(&media_source), &event_cb).unwrap();

        let camera =
            Camera { engine, device, media_source, event_rx, sample_rx, event_cb, sample_cb };
        camera.wait_for_event(CaptureEngineEvent::Initialized);
        camera.prepare_source_sink();
        camera
    }

    pub fn start(&self) {
        unsafe { self.engine.StartPreview().unwrap() }
    }

    pub fn stop(&self) {
        unsafe { self.engine.StopPreview().unwrap() }
    }

    pub fn wait_for_frame(&self) -> Option<Frame> {
        self.sample_rx
            .recv()
            .ok()
            .flatten()
            .and_then(|sample| {
                let Some(mt) = capture_engine_sink_get_media_type(&self.engine).ok() else {
                    return None;
                };
                let width = mt.frame_width();
                let height = mt.frame_height();
                sample_to_locked_buffer(&sample, width, height).ok()
            })
            .map(|sample| CameraFrame { sample })
            .map(|frame| Frame { frame })
    }
}

impl Camera {
    fn prepare_source_sink(&self) {
        capture_engine_prepare_sample_callback(&self.engine, &self.sample_cb).unwrap();
    }

    fn wait_for_event(&self, event: CaptureEngineEvent) {
        self.event_rx.iter().find(|e| e == &event);
    }
}

impl Frame {
    pub fn data(&self) -> FrameData {
        FrameData { data: self.frame.data() }
    }

    pub fn size_u32(&self) -> (u32, u32) {
        self.frame.size_u32()
    }
}

impl<'a> FrameData<'a> {
    pub fn data_u8(&self) -> &[u8] {
        self.data
    }

    pub fn data_u32(&self) -> &[u32] {
        let (a, data, b) = unsafe { self.data.align_to() };
        debug_assert!(a.is_empty());
        debug_assert!(b.is_empty());
        data
    }
}