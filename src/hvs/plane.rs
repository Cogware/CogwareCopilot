use alloc::vec;
use alloc::vec::Vec;
use log::info;
#[repr(C)]
#[derive(Clone)]
pub struct Plane {
    pub(super) format: PixelFormat,
    pub(super) order: PixelOrder,
    pub(super) start_x: u16,
    pub(super) start_y: u16,
    pub(super) width: u16,
    pub(super) height: u16,
    pub(super) pitch: u16,
    pub(super) framebuffer: Vec<u32>,
}

impl Plane {
    pub fn from_qoi(header: qoi::Header, image: Vec<u8>) -> Plane {
        let format = match header.channels {
            qoi::Channels::Rgb => PixelFormat::Rgb888,
            qoi::Channels::Rgba => PixelFormat::Rgba8888,
        };
        let img_pitch = match header.channels {
            qoi::Channels::Rgb => header.width * 3,
            qoi::Channels::Rgba => header.width * 4,
        };
        info!("IMAGE PITCH: {:?}", img_pitch);
        let framebuffer = image
            .chunks(4)
            .map(|p| u32::from_be_bytes([p[3], p[2], p[1], p[0]]))
            .collect::<Vec<_>>();

        Plane {
            format,
            order: PixelOrder::ARGB,
            start_x: 0,
            start_y: 0,
            width: 480,
            height: 480,
            pitch: img_pitch as u16,
            framebuffer,
        }
    }

    pub fn white() -> Plane {
        let framebuffer = vec![0xFFFFFFFF; 480 * 480];

        Plane {
            format: PixelFormat::Rgba8888,
            order: PixelOrder::ARGB,
            start_x: 0,
            start_y: 0,
            width: 480,
            height: 480,
            pitch: 32,
            framebuffer,
        }
    }

    pub fn green_half_alpha() -> Plane {
        let framebuffer = vec![0x3300FF00; 480 * 480];

        Plane {
            format: PixelFormat::Rgba8888,
            order: PixelOrder::ARGB,
            start_x: 0,
            start_y: 0,
            width: 480,
            height: 480,
            pitch: 32,
            framebuffer,
        }
    }

    pub fn set_pixel_format(self, format: PixelFormat) -> Plane {
        let mut plane = self;
        plane.format = format;
        plane
    }

    pub fn set_pixel_order(self, order: PixelOrder) -> Plane {
        let mut plane = self;
        plane.order = order;
        plane
    }

    pub fn set_start_x(self, start_x: u16) -> Plane {
        let mut plane = self;
        plane.start_x = start_x;
        plane
    }

    pub fn set_start_y(self, start_y: u16) -> Plane {
        let mut plane = self;
        plane.start_y = start_y;
        plane
    }

    pub fn set_width(self, width: u16) -> Plane {
        let mut plane = self;
        plane.width = width;
        plane
    }

    pub fn set_height(self, height: u16) -> Plane {
        let mut plane = self;
        plane.height = height;
        plane
    }

    pub fn set_pitch(self, pitch: u16) -> Plane {
        let mut plane = self;
        plane.pitch = pitch;
        plane
    }

    pub fn set_framebuffer(self, framebuffer: Vec<u32>) -> Plane {
        let mut plane = self;
        plane.framebuffer = framebuffer;
        plane
    }
}

/// The format of the pixels stored in the framebuffer.
#[derive(Copy, Clone)]
pub enum PixelFormat {
    /* 8bpp */
    Rgb332 = 0,

    /* 16bpp */
    Rgba4444 = 1,
    Rgb555 = 2,
    Rgba5551 = 3,
    Rgb565 = 4,

    /* 24bpp */
    Rgb888 = 5,
    Rgba6666 = 6,

    /* 32bpp */
    Rgba8888 = 7,
}

impl PixelFormat {
    fn depth(&self) -> u16 {
        match self {
            Self::Rgb332 => 8,
            Self::Rgba4444 | Self::Rgb555 | Self::Rgba5551 | Self::Rgb565 => 16,
            Self::Rgb888 | Self::Rgba6666 => 24,
            Self::Rgba8888 => 32,
        }
    }
}

impl Default for PixelFormat {
    fn default() -> Self {
        Self::Rgba8888
    }
}

/// The order of the pixels in the framebuffer.
///
/// NOTE: It seems the HVS needs the Alpha component to always come first, so either ARGB or AGBR
/// should be used.
#[derive(Copy, Clone)]
pub enum PixelOrder {
    RGBA = 0,
    BGRA = 1,
    ARGB = 2,
    ABGR = 3,
}

impl Default for PixelOrder {
    fn default() -> Self {
        Self::ARGB
    }
}
