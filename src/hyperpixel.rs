use bcm2837_hal::{
    delay::Timer,
    gpio::{Gpio, GpioExt, PinMode},
    pac,
};
use embedded_hal::delay::DelayNs;

const TICK_MICROS: u32 = 100;

#[derive(Debug)]
pub enum Error {
    FuckYou,
}

pub struct HyperPixel<'a> {
    gpio: Gpio,
    timer: &'a mut Timer,
}

impl<'a> HyperPixel<'a> {
    pub fn new(gpio: pac::GPIO, timer: &'a mut Timer) -> Self {
        let gpio = gpio.split();
        HyperPixel { gpio, timer }
    }

    pub fn init(mut self) {
        self.set_gpio_mode();
        self.init_display();
    }

    pub fn set_gpio_mode(&self) {
        self.gpio.pins[10..=11].iter().for_each(|p| {
            p.set_mode(PinMode::Output);
        });
        self.gpio.pins[18..=19].iter().for_each(|p| {
            p.set_mode(PinMode::Output).set_output_high();
        });

        self.gpio.pins[0..=9].iter().for_each(|p| {
            p.set_mode(PinMode::AF2);
        });
        self.gpio.pins[12..=17].iter().for_each(|p| {
            p.set_mode(PinMode::AF2);
        });
        self.gpio.pins[20..=25].iter().for_each(|p| {
            p.set_mode(PinMode::AF2);
        });
    }

    #[inline]
    fn set_clock_high(&self) {
        self.gpio.pins[11].set_output_high();
    }

    #[inline]
    fn set_clock_low(&self) {
        self.gpio.pins[11].set_output_low();
    }

    #[inline]
    fn pulse_clock(&mut self) {
        self.set_clock_low();
        self.tick();
        self.set_clock_high();
        self.tick();
    }

    #[inline]
    fn set_cs_high(&self) {
        self.gpio.pins[18].set_output_high();
    }

    #[inline]
    fn set_cs_low(&self) {
        //thank god for readable commands
        self.gpio.pins[18].set_output_low();
    }

    #[inline]
    fn set_mosi(&self, level: bool) {
        match level {
            true => self.gpio.pins[10].set_output_high(),
            false => self.gpio.pins[10].set_output_low(),
        };
    }

    #[inline]
    fn tick(&mut self) {
        self.timer.delay_us(TICK_MICROS);
        // sleep(Duration::from_micros(TICK_MICROS));
    }

    // anyway this is the horrible shit I had to do
    #[inline]
    fn write_bits(&mut self, by: u32, bit_count: u8) {
        for i in (0..bit_count).rev() {
            let gpio_level = (by >> i) & 1 != 0;
            self.set_mosi(gpio_level);
            self.pulse_clock();
        }

        self.set_mosi(false);
    }

    #[inline]
    fn write_command(&mut self, by: u32) {
        self.set_cs_low();
        self.write_bits(by, 9);
        self.set_cs_high();
    }

    #[inline]
    fn write_data(&mut self, reg: u32, bytes: &[u32]) {
        self.write_command(reg);

        for by in bytes {
            self.write_command(by | 0x100);
        }
    }

    #[inline]
    fn init_display(&mut self) {
        self.write_command(0x01);
        self.timer.delay_ms(240);

        self.write_data(0xFF, &[0x77, 0x01, 0x00, 0x00, 0x13]);
        self.write_data(0xEF, &[0x08]);
        self.write_data(0xFF, &[0x77, 0x01, 0x00, 0x00, 0x10]);

        self.write_data(0xC0, &[0x3B, 0x00]); // Scan line

        self.write_data(0xC1, &[0x0B, 0x02]); // VBP
        self.write_data(0xC2, &[0x00, 0x02]); // 07 OR 00
        self.write_data(0xCC, &[0x10]);

        // Gamma option B:
        //
        // Positive Voltage Gamma Control
        self.write_data(
            0xB0,
            &[
                0x00, 0x1D, 0x29, 0x12, 0x17, 0x0B, 0x18, 0x09, 0x08, 0x2A, 0x07, 0x14, 0x11, 0x27,
                0x32, 0x1F,
            ],
        );

        // Negative Voltage Gamma Control
        self.write_data(
            0xB1,
            &[
                0x00, 0x1D, 0x29, 0x12, 0x16, 0x0A, 0x18, 0x08, 0x09, 0x2A, 0x07, 0x13, 0x12, 0x27,
                0x33, 0x1F,
            ],
        );

        self.write_data(0xFF, &[0x77, 0x01, 0x00, 0x00, 0x11]);

        // VOP  3.5375+ *x 0.0125
        // 6D or 5D
        // or not, who fucking knows
        self.write_data(0xB0, &[0x9D]);

        // VCOM amplitude setting
        // 37 or 43
        // or not, who fucking knows
        self.write_data(0xB1, &[0x24]);

        // VGH Voltage setting
        // 12V
        self.write_data(0xB2, &[0x81]);
        self.write_data(0xB3, &[0x80]);

        // VGL Voltage setting
        // -8.3V
        self.write_data(0xB5, &[0x43]);
        self.write_data(0xB7, &[0x85]);
        self.write_data(0xB8, &[0x20]);
        self.write_data(0xC1, &[0x78]);
        self.write_data(0xC2, &[0x78]);

        self.write_data(0xE0, &[0x00, 0x00, 0x02]);
        self.write_data(
            0xE1,
            &[
                0x03, 0xA0, 0x00, 0x00, 0x04, 0xA0, 0x00, 0x00, 0x00, 0x20, 0x20,
            ],
        );

        self.write_data(
            0xE2,
            &[
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            ],
        );

        self.write_data(0xE3, &[0x00, 0x00, 0x11, 0x00]);
        self.write_data(0xE4, &[0x22, 0x00]);

        self.write_data(
            0xE5,
            &[
                0x05, 0xEC, 0xA0, 0xA0, 0x07, 0xEE, 0xA0, 0xA0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
        );

        self.write_data(0xE6, &[0x00, 0x00, 0x11, 0x00]);
        self.write_data(0xE7, &[0x22, 0x00]);
        self.write_data(
            0xE8,
            &[
                0x06, 0xED, 0xA0, 0xA0, 0x08, 0xEF, 0xA0, 0xA0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
        );

        self.write_data(0xEB, &[0x00, 0x00, 0x40, 0x40, 0x00, 0x00, 0x00]);

        self.write_data(
            0xED,
            &[
                0xFF, 0xFF, 0xFF, 0xBA, 0x0A, 0xBF, 0x45, 0xFF, 0xFF, 0x54, 0xFB, 0xA0, 0xAB, 0xFF,
                0xFF, 0xFF,
            ],
        );

        self.write_data(
            0xEF,
            &[
                0x10, 0x0D, 0x04, 0x08, // Positive Voltage Gamma Control
                0x3F, 0x1F,
            ],
        );

        self.write_data(0xFF, &[0x77, 0x01, 0x00, 0x00, 0x13]);
        self.write_data(0xE8, &[0x00, 0x0E]);
        self.write_data(0xFF, &[0x77, 0x01, 0x00, 0x00, 0x00]);
        self.write_data(0x11, &[]);
        self.write_data(0xCD, &[0x08]);
        self.write_data(0x36, &[0x08]);
        self.write_data(0x3A, &[0x66]);

        self.timer.delay_ms(120);

        self.write_data(0xFF, &[0x77, 0x01, 0x00, 0x00, 0x13]);
        self.write_data(0xE8, &[0x00, 0x0C]);

        self.timer.delay_ms(10);

        self.write_data(0xE8, &[0x00, 0x00]);
        self.write_data(0xFF, &[0x77, 0x01, 0x00, 0x00]);
        self.write_data(0x29, &[]);

        self.timer.delay_ms(20);
    }
}
