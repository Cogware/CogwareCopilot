use crate::mailbox::{SCREEN_HEIGHT, SCREEN_WIDTH};
use embedded_graphics::{pixelcolor::Rgb666, prelude::PixelColor};
use noto_sans_mono_bitmap::{get_raster, get_raster_width, FontWeight, RasterHeight};

const LETTER_FONT_WEIGHT: FontWeight = FontWeight::Regular;
const LETTER_FONT_HEIGHT: RasterHeight = RasterHeight::Size16;
pub const LETTER_WIDTH: usize = get_raster_width(LETTER_FONT_WEIGHT, LETTER_FONT_HEIGHT);
const BOOT_IMAGE_QOI: &[u8] = include_bytes!("CogWare.qoi");

pub trait FrameBufferInterface {
    fn draw_rect_fill(&mut self, point: &Coordinates, width: u32, height: u32, color: Color) {
        let width = width as usize;
        let height = height as usize;
        for y in 0..height {
            for x in 0..width {
                self.use_pixel(point.x_usize() + x, point.y_usize() + y, color);
            }
        }
    }

    fn write_char(&mut self, c: char, coordinates: Coordinates, color: Color) {
        let char_raster =
            get_raster(c, LETTER_FONT_WEIGHT, LETTER_FONT_HEIGHT).expect("unsupported char");
        for (row_i, row) in char_raster.raster().iter().enumerate() {
            for (col_i, pixel) in row.iter().enumerate() {
                let actual_color = if pixel.count_zeros() == 8 {
                    BLACK_COLOR
                } else {
                    color
                };
                self.use_pixel(
                    coordinates.x_usize() + col_i,
                    coordinates.y_usize() + row_i,
                    actual_color,
                );
            }
        }
    }

    /// [x,y] the top left center
    fn draw_rect(&mut self, point: Coordinates, width: u32, height: u32, color: Color) {
        let width = width as usize;
        let height = height as usize;
        for y in 0..height {
            self.use_pixel(point.x_usize(), point.y_usize() + y, color);
            self.use_pixel(point.x_usize() + width, point.y_usize() + y, color);
        }
        for x in 0..width {
            self.use_pixel(point.x_usize() + x, point.y_usize(), color);
            self.use_pixel(point.x_usize() + x, point.y_usize() + height, color);
        }
    }

    fn raw_buffer(&mut self) -> &mut [u32];
    fn width(&self) -> usize {
        self.width_u32() as usize
    }
    fn width_u32(&self) -> u32 {
        SCREEN_WIDTH
    }
    fn height_u32(&self) -> u32 {
        SCREEN_HEIGHT
    }
    fn height(&self) -> usize {
        self.height_u32() as usize
    }

    fn use_pixel(&mut self, x_usize: usize, y_usize: usize, color: Color) {
        let width = self.width();
        self.raw_buffer()[width * y_usize + x_usize] = color.rgb();
    }

    fn display_boot_image(&mut self) {
        let (_header, decoded) = qoi::decode_to_vec(BOOT_IMAGE_QOI).unwrap();

        decoded
            .chunks(4)
            .map(|p| u32::from_be_bytes([p[3], p[2], p[1], p[0]]))
            .enumerate()
            .for_each(|(i, p)| self.raw_buffer()[i] = p);
    }

    fn display_image(&mut self, top_left: &Coordinates, image: &[u32], width: u32, height: u32) {
        let fb_width = self.width();
        let width = width as usize;
        for y in 0..height as usize {
            for x in 0..width {
                let pos: usize = y * width + x;
                let (x, y) = (x + top_left.x_usize(), y + top_left.y_usize());
                let index = fb_width * y + x;
                self.raw_buffer()[index] = image[pos];
            }
        }
    }
    fn clear_screen(&mut self) {
        for i in self.raw_buffer().iter_mut() {
            *i = 0;
        }
    }

    // draw the local buffer of the framebuffer to the screen
    fn update(&mut self);
}

#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct Color {
    pub(crate) rgb: u32,
}

impl Color {
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self {
            rgb: Self::rgb_u32(red, green, blue),
        }
    }

    const fn rgb_u32(red: u8, green: u8, blue: u8) -> u32 {
        (255 << 28 | (red as u32) << 16) | ((green as u32) << 8) | (blue as u32)
    }
    // inlined to increase performance by 5~ ms per loop
    #[inline(always)]
    pub fn rgb(&self) -> u32 {
        self.rgb
    }
}

pub const BLACK_COLOR: Color = Color::new(0, 0, 0);
pub const WHITE_COLOR: Color = Color::new(255, 255, 255);

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Coordinates {
    pub virtual_x: f64,
    pub virtual_y: f64,
}

impl Coordinates {
    #[inline(always)]
    pub const fn new(x: u32, y: u32) -> Self {
        Self {
            virtual_x: x as f64,
            virtual_y: y as f64,
        }
    }
    #[inline(always)]
    pub fn x(&self) -> u32 {
        self.virtual_x as u32
    }
    #[inline(always)]
    pub fn y(&self) -> u32 {
        self.virtual_y as u32
    }

    #[inline(always)]
    pub fn x_usize(&self) -> usize {
        self.virtual_x as usize
    }

    #[inline(always)]
    pub fn y_usize(&self) -> usize {
        self.virtual_y as usize
    }

    pub fn add_virtual_x(&mut self, x: f64) {
        self.virtual_x += x;
    }

    pub fn sub_virtual_x(&mut self, x: f64) {
        self.virtual_x -= x;
    }

    pub fn set_virtual_x(&mut self, x: f64) {
        self.virtual_x = x;
    }

    pub fn sub_virtual_y(&mut self, speed: f64, delta: u64) {
        self.virtual_y -= speed * delta as f64;
    }

    pub fn add_virtual_y(&mut self, speed: f64, delta: u64) {
        self.virtual_y += speed * delta as f64;
    }
}


